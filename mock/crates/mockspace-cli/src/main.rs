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

use clap::{Parser, Subcommand};

use mockspace_rs::bootstrap;

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

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}
