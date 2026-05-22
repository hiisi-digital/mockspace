//! `mock` binary entry point.
//!
//! Per v2 spec §57 the CLI is the user-facing surface that wires
//! through the v2 modules under `mockspace_rs`. This first slice
//! ships the bootstrap-consuming subcommands (`status`, `install`,
//! `uninstall`, `refresh`); engine-side subcommands (`check`,
//! `lock`, `unlock`, `deprecate`, `close`, `phase`, `task`) land
//! as follow-up slices.
//!
//! Cargo alias: the bootstrap installs `[alias] mock = "run
//! --manifest-path mock/Cargo.toml --bin mock --"` so `cargo mock
//! <subcommand>` resolves here once the bootstrap runs.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use mockspace_core::lint::{Gate, LintCfgStore, LintEngine, RunSurface, Severity};
use mockspace_rs::{
    bootstrap,
    config_loader::{find_and_read_lints_toml, LintsConfig, LintsTomlFile, OverrideCascade},
    engine::MockspaceEngine,
    explain, preset_source,
};

/// Empty `LintCfgStore` for the `cargo mock check` CLI. The lint
/// engine consults this for per-lint runtime severity overrides; an
/// empty store means "fall back to the lint's `default_severity()`",
/// which the `LintsConfig::load` cascade already populated with the
/// fully merged value. Future CLI work that adds runtime `--severity
/// <lint>=<value>` overrides can replace this with a richer store.
struct EmptyCfgStore;

impl LintCfgStore for EmptyCfgStore {
    fn get(&self, _lint_name: &str) -> Option<&toml::Table> {
        None
    }
}

/// Top-level CLI shape. Subcommands branch per intent; the
/// `--repo-root` global flag overrides the default (cwd) so the
/// CLI is testable against a fixture directory without changing
/// the process working directory.
#[derive(Parser, Debug)]
#[command(name = "mock", version, about = "mockspace v2 command-line surface", long_about = None)]
struct Cli {
    /// Repo root the subcommand operates against. Defaults to the
    /// current working directory.
    #[arg(long, global = true)]
    repo_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Report v2 adoption status for the repo. Read-only.
    Status,
    /// Install the v2 bootstrap: writes `[alias] mock = ...` and
    /// configures git hooks under `mock/target/hooks/`.
    Install,
    /// Remove the v2 bootstrap state: clears the cargo alias and
    /// the git hooks.
    Uninstall,
    /// Re-derive the v2 bootstrap state. Functionally identical
    /// to `install`; named separately so drift-repair has a
    /// distinct command-line affordance.
    Refresh,
    /// Explain how a lint resolves through the cascade. Prints the
    /// per-layer contributions (catalog defaults, preset chain,
    /// workspace defaults, per-lint TOML, CLI overrides) plus the
    /// final per-field winners.
    Explain {
        /// Lint name to explain. Must match a name registered in the
        /// catalog; misspellings surface `LintNotFound`.
        name: String,
    },
    /// Run the lint engine against the repo. Prints findings to
    /// stdout in the `<file>:<line>:<col>: [<severity>] <name>:
    /// <message>` format. Exits non-zero if any finding is
    /// classified as error at the chosen gate.
    Check {
        /// Severity gate to evaluate at. `commit` is the lightest
        /// (pre-commit hook), `build` is intermediate (CI), `push`
        /// is the strictest. Defaults to `commit`.
        #[arg(long, value_enum, default_value_t = GateArg::Commit)]
        gate: GateArg,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum GateArg {
    Commit,
    Build,
    Push,
}

impl From<GateArg> for Gate {
    fn from(g: GateArg) -> Gate {
        match g {
            GateArg::Commit => Gate::Commit,
            GateArg::Build => Gate::Build,
            GateArg::Push => Gate::Push,
        }
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let repo_root = match cli.repo_root {
        Some(p) => p,
        None => match std::env::current_dir() {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "cannot determine current directory: {e}. Pass --repo-root explicitly."
                );
                return std::process::ExitCode::FAILURE;
            }
        },
    };

    match cli.command {
        Command::Status => run_status(&repo_root),
        Command::Install => run_install(&repo_root),
        Command::Uninstall => run_uninstall(&repo_root),
        Command::Refresh => run_refresh(&repo_root),
        Command::Explain { name } => run_explain(&repo_root, &name),
        Command::Check { gate } => run_check(&repo_root, gate.into()),
    }
}

fn run_status(repo_root: &std::path::Path) -> std::process::ExitCode {
    let s = bootstrap::status(repo_root);
    let summary = if s.is_fully_adopted() {
        "fully adopted"
    } else if s.is_uninstalled() {
        "not installed"
    } else {
        "partial adoption"
    };
    println!("mockspace v2 adoption: {summary}");
    println!("  mock/ directory       : {}", yes_no(s.has_mock_dir));
    println!("  cargo alias `mock`    : {}", yes_no(s.has_cargo_alias));
    println!("  core.hooksPath set    : {}", yes_no(s.has_hooks_path));
    std::process::ExitCode::SUCCESS
}

fn run_install(repo_root: &std::path::Path) -> std::process::ExitCode {
    match bootstrap::install(repo_root) {
        Ok(bootstrap::InstallOutcome::Installed) => {
            println!("v2 bootstrap installed at {}", repo_root.display());
            std::process::ExitCode::SUCCESS
        }
        Ok(bootstrap::InstallOutcome::AlreadyInstalled) => {
            println!("v2 bootstrap already installed; no changes");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("install failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_uninstall(repo_root: &std::path::Path) -> std::process::ExitCode {
    match bootstrap::uninstall(repo_root) {
        Ok(bootstrap::UninstallOutcome::Removed) => {
            println!("v2 bootstrap removed from {}", repo_root.display());
            std::process::ExitCode::SUCCESS
        }
        Ok(bootstrap::UninstallOutcome::AlreadyUninstalled) => {
            println!("v2 bootstrap was not installed; no changes");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("uninstall failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_refresh(repo_root: &std::path::Path) -> std::process::ExitCode {
    match bootstrap::refresh(repo_root) {
        Ok(bootstrap::InstallOutcome::Installed) => {
            println!("v2 bootstrap refreshed at {}", repo_root.display());
            std::process::ExitCode::SUCCESS
        }
        Ok(bootstrap::InstallOutcome::AlreadyInstalled) => {
            println!("v2 bootstrap state matches canonical; no changes");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("refresh failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Run the explain subcommand. Walks the cascade for the named lint
/// and prints the structured report. Reads the consumer's
/// mockspace.toml (or `lints.toml` / `mock/lints.toml` per the
/// canonical search order in [`find_and_read_lints_toml`]) from
/// `repo_root`; an absent or unparseable file falls back to an empty
/// [`LintsTomlFile::default()`] so the cascade walk still surfaces
/// the catalog defaults. CLI-side overrides (Layer 5) and the
/// workspace-defaults intermediate layer (Layer 3) are still empty
/// for this slice and land separately.
fn run_explain(repo_root: &std::path::Path, lint_name: &str) -> std::process::ExitCode {
    let user_toml = match find_and_read_lints_toml(repo_root) {
        Ok(toml) => toml,
        Err(e) => {
            // Parse failures are loud but non-fatal for explain:
            // print a warning then proceed with the default empty
            // file. This keeps explain runnable when the consumer's
            // TOML has unrelated breakage; a `cargo mock check`
            // would still surface the parse error and block.
            eprintln!(
                "warning: could not read user lints TOML ({e}); proceeding with catalog defaults only"
            );
            LintsTomlFile::default()
        }
    };
    let overrides = OverrideCascade::default();
    let source = preset_source::FirstPartyPresetSource::new();
    match explain::explain_lint(lint_name, &user_toml, &overrides, &source) {
        Ok(report) => {
            print_explain_report(&report);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("explain failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn print_explain_report(report: &explain::ExplainReport) {
    println!("lint: {}", report.lint_name);
    println!("primitive: {}", report.primitive_kind);
    println!(
        "catalog severity: commit={:?} build={:?} push={:?}",
        report.catalog_severity.commit,
        report.catalog_severity.build,
        report.catalog_severity.push,
    );
    println!();
    println!("Cascade layers:");
    for layer in &report.layers {
        match &layer.source {
            Some(src) => println!("  {} ({})", layer.label, src),
            None => println!("  {}", layer.label),
        }
        for (key, value) in &layer.config {
            println!("    config.{key} = {value}");
        }
        for (key, value) in &layer.scope {
            println!("    scope.{key} = {value}");
        }
    }
    if !report.final_values.is_empty() {
        println!();
        println!("Final values:");
        for entry in &report.final_values {
            // `winning_label` already starts with "Layer N:" so we
            // don't prefix `layer N:` ahead of it; that would print
            // as the doubled "layer 4: Layer 4: per-lint TOML" shape.
            println!(
                "  {} = {} ({})",
                entry.field_path, entry.value, entry.winning_label
            );
        }
    }
}

/// Run the lint engine against `repo_root` at the given `gate`.
/// Composes the four engine pieces:
///
/// 1. `LintsConfig::load(repo_root, OverrideCascade::default())` reads
///    the user TOML + applies cascade (Layer 5 CLI overrides remain
///    empty for this slice; flagged for a follow-up).
/// 2. `MockspaceEngine::new()` instantiates the catalog-default lint
///    set.
/// 3. `engine.scope_project(repo_root, RunSurface::Local)` walks the
///    project tree and parses the relevant documents.
/// 4. `engine.run(&project, gate, &cfg)` produces a `Vec<Finding>`.
///
/// Findings render one per line to stdout. Exit code is FAILURE iff
/// any finding's severity is `Error` at the chosen gate (matching
/// the pre-commit / pre-push hook gate semantics).
fn run_check(repo_root: &std::path::Path, gate: Gate) -> std::process::ExitCode {
    let cfg_obj = match LintsConfig::load(repo_root, OverrideCascade::default()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("check failed: could not load lints config: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // The cascade-resolved entries carry the merged severities and
    // configs; the engine reads them at instantiation time. The
    // `cfg` argument to `engine.run` covers per-lint runtime
    // overrides, which the CLI does not surface yet.
    let engine = MockspaceEngine::with_entries(cfg_obj.entries);
    let project = match engine.scope_project(repo_root, RunSurface::Local) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("check failed: could not scope project: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let cfg_store = EmptyCfgStore;
    let findings = match engine.run(&project, gate, &cfg_store) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("check failed: engine dispatch error: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    if findings.is_empty() {
        println!("no findings at gate {gate:?}");
        return std::process::ExitCode::SUCCESS;
    }

    let mut had_error = false;
    for f in &findings {
        let path = f.span.file.display();
        let line = f.span.start_line;
        let col = f.span.start_column;
        let sev = format!("{:?}", f.severity);
        println!(
            "{path}:{line}:{col}: [{sev}] {name}: {msg}",
            name = f.lint_name,
            msg = f.message
        );
        if matches!(f.severity, Severity::Error) {
            had_error = true;
        }
    }
    if had_error {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
