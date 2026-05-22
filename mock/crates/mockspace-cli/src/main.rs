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

use mockspace_rs::{
    bootstrap,
    config_loader::{find_and_read_lints_toml, LintsConfig, LintsTomlFile, OverrideCascade},
    design_rounds::discover_design_rounds,
    engine::MockspaceEngine,
    explain, preset_source, AdvanceError, AdvanceReport, AdvanceVerb, ArchiveError,
    ArchiveReport, DesignRound, FlockTransitionLock, Gate, LintCfgStore, LintEngine, LockError,
    ObjectId, Phase, RepoError, RepoHandle, ReplanMode, RoundState, RunSurface, Severity, Slug,
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
        /// Emit findings as a pretty-printed JSON array on stdout
        /// instead of the human-readable diagnostic format. The
        /// exit code semantics stay the same (failure on any Error
        /// at the gate); the format suits machine consumers such
        /// as editor integrations and CI dashboards.
        #[arg(long)]
        json: bool,
        /// Run surface to scope the project under. `local` is the
        /// default and matches developer-machine usage. `ci`
        /// simulates a CI run (useful for reproducing CI-only lint
        /// outcomes locally). `editor` is the LSP-like surface for
        /// editor integrations.
        #[arg(long, value_enum, default_value_t = SurfaceArg::Local)]
        surface: SurfaceArg,
    },
    /// Run a phase-transition verb against a round. The four
    /// verbs follow spec §14: `plan` opens TOPIC -> PLAN(doc),
    /// `apply` seals PLAN(side) -> APPLY(side) and captures the
    /// anchor, `finish` advances APPLY(doc) -> PLAN(src) or
    /// APPLY(src) -> DONE, `replan` deprecates the locked
    /// manifest and returns to PLAN(side).
    Phase {
        #[command(subcommand)]
        verb: PhaseVerb,
    },
    /// Archive a DONE round to `refs/mock/round-archive` and
    /// delete the source `refs/mock/round/<slug>`. Spec §26.
    Close {
        /// Slug of the round to archive.
        slug: String,
    },
    /// Walk the v1 mockspace state under `mock/design_rounds/`
    /// and print a per-round migration report plus a checklist
    /// of human-side updates (agent docs, hook configs that
    /// reference the old subcommand surface) needed to finish
    /// the v1 -> v2 transition.
    ///
    /// This is a guide. The auto-conversion (writing v2 ref
    /// trees from v1 filesystem state) lands in a follow-up
    /// behind an explicit `--auto` flag once the classification
    /// surface here is reviewed.
    Migrate,
}

#[derive(Subcommand, Debug)]
enum PhaseVerb {
    /// Open a planning surface on a TOPIC-phase round. Rewrites
    /// `.phase` to `plan_doc`; the manifest authoring itself
    /// happens via separate ref edits.
    Plan {
        /// Slug of the round to open PLAN(doc) on. Must currently
        /// be in TOPIC phase.
        slug: String,
    },
    /// Seal the authoring manifest and transition PLAN(side) ->
    /// APPLY(side). Requires the source-side branch tip OID as
    /// a hex SHA so the anchor capture sees a stable input.
    Apply {
        /// Slug of the round to seal. Must currently be in
        /// PLAN(doc) or PLAN(src) phase.
        slug: String,
        /// Source-side branch tip OID at APPLY entry, in hex
        /// form (e.g. the output of `git rev-parse HEAD`). The
        /// anchor records this OID for provenance.
        #[arg(long)]
        source_tip: String,
    },
    /// Advance the round past APPLY: APPLY(doc) -> PLAN(src), or
    /// APPLY(src) -> DONE. The doc-side locked manifest is
    /// preserved through the doc-to-src transition.
    Finish {
        /// Slug of the round to advance. Must currently be in
        /// APPLY(doc) or APPLY(src) phase.
        slug: String,
    },
    /// Deprecate the locked manifest and return APPLY(side) to
    /// PLAN(side). The locked manifest is renamed to the next
    /// `manifest.<side>.deprecated.<n>.toml` slot.
    Replan {
        /// Slug of the round to replan. Must currently be in
        /// APPLY(doc) or APPLY(src) phase.
        slug: String,
        /// Replan mode. Currently the local-ref portion does not
        /// branch on mode (rename plus phase flip is identical
        /// across modes); the parameter exists for API stability
        /// and forwards to higher orchestration when wired.
        #[arg(long, value_enum, default_value_t = ReplanModeArg::Destructive)]
        mode: ReplanModeArg,
        /// Claimed source-side file path that may have lost
        /// post-APPLY work. Repeatable. Only consulted when
        /// `--mode accept-loss`; ignored otherwise. Each occurrence
        /// adds one path to the accept-loss list.
        #[arg(long = "accept-loss-path", value_name = "PATH")]
        accept_loss_paths: Vec<std::path::PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ReplanModeArg {
    /// Overwrite source-side files at restoration time.
    /// Refuses if post-APPLY commits touch claimed files.
    Destructive,
    /// Commit the restoration on top of post-APPLY state.
    AdditiveByCommit,
    /// Accept post-APPLY work loss for the paths supplied via
    /// `--accept-loss-path`. Other claimed files refuse as in
    /// `destructive`.
    AcceptLoss,
}

/// Convert a CLI mode + the optional `--accept-loss-path` list
/// into the core's `ReplanMode`. The list is only consulted for
/// the `accept-loss` variant; other variants ignore it.
fn replan_mode_from(arg: ReplanModeArg, accept_loss_paths: Vec<std::path::PathBuf>) -> ReplanMode {
    match arg {
        ReplanModeArg::Destructive => ReplanMode::Destructive,
        ReplanModeArg::AdditiveByCommit => ReplanMode::AdditiveByCommit,
        ReplanModeArg::AcceptLoss => ReplanMode::AcceptRestorationLoss(accept_loss_paths),
    }
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SurfaceArg {
    Local,
    Ci,
    Editor,
}

impl From<SurfaceArg> for RunSurface {
    fn from(s: SurfaceArg) -> RunSurface {
        match s {
            SurfaceArg::Local => RunSurface::Local,
            SurfaceArg::Ci => RunSurface::Ci,
            SurfaceArg::Editor => RunSurface::Editor,
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
        Command::Check { gate, json, surface } => {
            run_check(&repo_root, gate.into(), json, surface.into())
        }
        Command::Phase { verb } => run_phase(&repo_root, verb),
        Command::Close { slug } => run_close(&repo_root, &slug),
        Command::Migrate => run_migrate(&repo_root),
    }
}

/// Parse a slug string into [`Slug`] with a CLI-friendly error
/// message. The `Slug::new` validation rejects empty, too-long,
/// and out-of-charset inputs.
fn parse_slug(raw: &str) -> Result<Slug, String> {
    Slug::new(raw).map_err(|e| format!("invalid slug `{raw}`: {e}"))
}

/// Open the repo handle from `repo_root` with a CLI-friendly error
/// message. `RepoError::NotFound` is rendered as "not inside a git
/// repository"; other variants surface their inner error.
fn open_repo(repo_root: &std::path::Path) -> Result<RepoHandle, String> {
    RepoHandle::open(repo_root).map_err(|e| match e {
        RepoError::NotFound { .. } => format!(
            "not inside a git repository: no .git directory found under `{}`",
            repo_root.display()
        ),
        other => format!("cannot open repository at `{}`: {other}", repo_root.display()),
    })
}

/// Acquire the transition lock at `<repo_root>/.git/mockspace/.lock`
/// with a CLI-friendly error message. Blocking acquire so the user
/// waits for any concurrent process to release.
///
/// `LockError::AlreadyHeld` is unreachable here because `acquire`
/// is blocking. Only `try_acquire` ever surfaces it; this CLI uses
/// the blocking path. The match still covers the variant defensively
/// in case a future caller switches to `try_acquire`, but the body
/// of those arms is currently unreachable in practice.
fn acquire_lock(repo_root: &std::path::Path) -> Result<FlockTransitionLock, String> {
    FlockTransitionLock::acquire(repo_root).map_err(|e| match e {
        LockError::GitDirMissing { workspace_root } => format!(
            "cannot acquire transition lock: no .git directory at `{}`",
            workspace_root.display()
        ),
        LockError::AlreadyHeld { previous: Some(h) } => format!(
            "transition lock held by host={} pid={} acquired_at={}",
            h.hostname, h.pid, h.acquired_at
        ),
        LockError::AlreadyHeld { previous: None } => {
            "transition lock held by an unknown process".to_owned()
        }
        LockError::Io { during, path, error } => format!(
            "transition lock IO failed during {during} on `{}`: {error}",
            path.display()
        ),
    })
}

fn run_phase(
    repo_root: &std::path::Path,
    verb: PhaseVerb,
) -> std::process::ExitCode {
    // Parse the per-verb inputs FIRST so a malformed slug or
    // source-tip bails out before we touch the repo or hold the
    // transition lock. (The lock release on error path is
    // automatic via Drop, but skipping the acquire is cheaper.)
    let (slug_raw, advance_verb) = match verb {
        PhaseVerb::Plan { slug } => (slug, AdvanceVerb::Plan),
        PhaseVerb::Apply { slug, source_tip } => {
            let oid = match ObjectId::from_hex(source_tip.as_bytes()) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!(
                        "invalid --source-tip `{source_tip}`: {e}. Expected a hex SHA \
                         (40 chars for SHA-1, 64 for SHA-256)."
                    );
                    return std::process::ExitCode::FAILURE;
                }
            };
            (
                slug,
                AdvanceVerb::Apply {
                    source_branch_tip: oid,
                },
            )
        }
        PhaseVerb::Finish { slug } => (slug, AdvanceVerb::Finish),
        PhaseVerb::Replan {
            slug,
            mode,
            accept_loss_paths,
        } => (
            slug,
            AdvanceVerb::Replan(replan_mode_from(mode, accept_loss_paths)),
        ),
    };

    let slug = match parse_slug(&slug_raw) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let handle = match open_repo(repo_root) {
        Ok(h) => h,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let lock = match acquire_lock(repo_root) {
        Ok(l) => l,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match handle.advance_phase(&lock, &slug, advance_verb) {
        Ok(report) => {
            print_advance_report(&slug_raw, &report);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("phase transition failed: {}", render_advance_error(&e));
            std::process::ExitCode::FAILURE
        }
    }
}

fn print_advance_report(slug_raw: &str, report: &AdvanceReport) {
    println!(
        "round `{slug_raw}` -> phase `{phase}` via verb `{verb:?}`",
        phase = phase_marker(report.landed_in),
        verb = report.verb,
    );
    println!("  new commit: {}", report.new_commit);
    if let Some(seal) = &report.seal {
        println!("  locked manifest: {}", seal.locked_manifest_path);
        println!("  anchor blobs: {}", seal.anchor_blob_count);
    }
    if let Some(n) = report.deprecated_iteration {
        println!("  deprecated as iteration {n}");
    }
}

fn phase_marker(phase: Phase) -> &'static str {
    match phase {
        Phase::Topic => "topic",
        Phase::PlanDoc => "plan_doc",
        Phase::ApplyDoc => "apply_doc",
        Phase::PlanSrc => "plan_src",
        Phase::ApplySrc => "apply_src",
        Phase::Done => "done",
    }
}

fn render_advance_error(e: &AdvanceError) -> String {
    match e {
        AdvanceError::RoundRefMissing { slug } => {
            format!("no round ref exists for slug `{slug}`")
        }
        AdvanceError::InvalidFromPhase {
            verb,
            current,
            allowed_from,
        } => format!(
            "verb {verb:?} is not valid from phase {current:?}; allowed from {allowed_from:?}"
        ),
        other => format!("{other}"),
    }
}

fn run_close(repo_root: &std::path::Path, slug_raw: &str) -> std::process::ExitCode {
    // Parse the slug first so a malformed input bails out before
    // any repo or lock work happens.
    let slug = match parse_slug(slug_raw) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let handle = match open_repo(repo_root) {
        Ok(h) => h,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let lock = match acquire_lock(repo_root) {
        Ok(l) => l,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match handle.archive_round(&lock, &slug) {
        Ok(report) => {
            print_archive_report(slug_raw, &report);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("archive failed: {}", render_archive_error(&e));
            std::process::ExitCode::FAILURE
        }
    }
}

fn print_archive_report(slug_raw: &str, report: &ArchiveReport) {
    println!("round `{slug_raw}` archived to refs/mock/round-archive");
    println!("  new archive commit: {}", report.archive_commit);
    println!("  entries archived: {}", report.entries_archived);
    if report.source_ref_deleted {
        println!("  source ref `refs/mock/round/{slug_raw}` deleted");
    } else {
        println!(
            "  WARNING: source ref `refs/mock/round/{slug_raw}` NOT deleted; re-run is idempotent"
        );
        if let Some(err) = &report.source_delete_error {
            println!("  delete error: {err}");
        }
    }
}

fn render_archive_error(e: &ArchiveError) -> String {
    match e {
        ArchiveError::RoundRefMissing { slug } => {
            format!("no round ref exists for slug `{slug}`")
        }
        ArchiveError::NotDone { current } => {
            format!("round is in phase {current:?}; only DONE rounds may be archived")
        }
        other => format!("{other}"),
    }
}

/// Walk `mock/design_rounds/` and print a per-round migration
/// report. Each entry classifies the v1 state, names the target
/// v2 phase, and gives concrete next-step instructions for what
/// the human must do. After the per-round table, prints a
/// checklist of project-wide updates (agent docs, hook configs)
/// that the tool does not attempt to mutate automatically.
fn run_migrate(repo_root: &std::path::Path) -> std::process::ExitCode {
    let view = discover_design_rounds(repo_root);
    if view.rounds.is_empty() {
        println!(
            "no v1 round directories found under `{}/mock/design_rounds/`",
            repo_root.display()
        );
        println!();
        print_migration_postscript();
        return std::process::ExitCode::SUCCESS;
    }

    println!(
        "v1 mockspace state discovered under `{}/mock/design_rounds/`:",
        repo_root.display()
    );
    println!("  {} round(s) to migrate", view.rounds.len());
    println!();
    println!("Per-round migration plan:");
    println!();
    for round in &view.rounds {
        print_round_migration_report(round);
        println!();
    }
    print_migration_postscript();
    std::process::ExitCode::SUCCESS
}

fn print_round_migration_report(round: &DesignRound) {
    println!("  round `{}`:", round.timestamp);
    println!("    v1 state:    {:?}", round.state);
    if let Some(doc_cl) = &round.doc_cl {
        println!("    doc CL:      `{}`", doc_cl.display());
    }
    if let Some(src_cl) = &round.src_cl {
        println!("    src CL:      `{}`", src_cl.display());
    }
    println!("    locked:      {}", round.locked);

    let (target_phase, action) = classify_v1_round(round);
    println!("    target v2:   phase = `{target_phase}`");
    println!("    action:      {action}");
}

/// Map a v1 [`DesignRound`] to (target v2 phase marker, human
/// action narrative). The mapping is conservative: states that
/// can be auto-constructed from the v1 filesystem alone print a
/// "tool can write" hint; states that require human authoring
/// (e.g., translating markdown CLs to v2 structured TOML
/// manifests) print explicit manual steps.
fn classify_v1_round(round: &DesignRound) -> (&'static str, String) {
    match round.state {
        RoundState::Topic => (
            "topic",
            "Empty v1 round (no CLs). Tool can construct the v2 ref directly: \
             run `mock phase plan <slug>` once `mock migrate --auto` ships, or \
             manually push an orphan ref carrying `.phase = topic`."
                .to_owned(),
        ),
        RoundState::Doc => (
            "plan_doc",
            "v1 doc CL exists in markdown form. v2 stores manifests as \
             structured TOML (see `mock/crates/mockspace-core/src/manifest.rs::Manifest`), \
             so the doc CL is NOT 1:1 translatable. Manual step: author \
             `manifest.doc.toml` for the round, then run `mock phase plan <slug>` \
             plus follow-up writes to seat the manifest."
                .to_owned(),
        ),
        RoundState::Src => (
            "plan_src",
            "v1 round advanced to src side; doc was previously sealed. \
             Manual steps: author both `manifest.doc.locked.toml` (from the \
             prior doc CL) and `manifest.src.toml` (from the current src CL), \
             then drive the round through `mock phase apply` (doc) + \
             `mock phase finish` to land at PLAN(src)."
                .to_owned(),
        ),
        RoundState::Locked => {
            // Infer the side from which CLs are present. v1 stores
            // them as `Option<PathBuf>` so both being present means
            // doc was already sealed and src is the live work
            // (=> apply_src); only doc present means the lock is on
            // the doc side (=> apply_doc).
            let (target_phase, side_marker) = match (&round.doc_cl, &round.src_cl) {
                (_, Some(_)) => ("apply_src", "src"),
                (Some(_), None) => ("apply_doc", "doc"),
                (None, None) => ("apply_<side>", "<side>"),
            };
            (
                target_phase,
                format!(
                    "v1 round has a locked CL. v2 APPLY phase requires a captured \
                 anchor; v1 carries no equivalent record. Manual steps: author \
                 `manifest.{side_marker}.locked.toml` from the locked CL, run \
                 `mock phase apply <slug> --source-tip <hex>` to capture a \
                 fresh anchor against the current source-side branch tip."
                ),
            )
        }
        RoundState::Closed => (
            "done (then archived)",
            "v1 closed round. Manual steps: reconstruct the v2 round-ref \
             tree carrying `.phase = done` plus the locked manifests for \
             both sides, then `mock close <slug>` to move it into \
             `refs/mock/round-archive`."
                .to_owned(),
        ),
    }
}

fn print_migration_postscript() {
    println!("Things you (the consumer) need to update by hand:");
    println!();
    println!("  CI workflows (`.github/workflows/*.yml`, equivalent on other forges):");
    println!("    If your CI invokes `cargo mock lock` or `cargo mock deprecate`,");
    println!("    switch the calls to the v2 verb surface:");
    println!("      `cargo mock lock <slug>`        -> `cargo mock phase apply <slug> --source-tip <hex>`");
    println!("      `cargo mock deprecate <slug>`   -> `cargo mock phase replan <slug> [--mode ...]`");
    println!("      `cargo mock close <slug>`       -> (unchanged in v2)");
    println!();
    println!("  Tracked tasks / project memos that reference v1 verbs:");
    println!("    Grep your repo for `mock lock` / `mock deprecate` in prose and");
    println!("    update by hand. Mockspace does not edit your in-repo writing.");
    println!();
    println!("Things mockspace ships builtin (do NOT update by hand):");
    println!();
    println!("  Canonical agent rules describing mockspace itself (what the phases");
    println!("  are, what the verbs do, the workflow shape) and the canonical hook");
    println!("  scripts that drive `cargo mock check` at the commit/build/push gates");
    println!("  are part of the mockspace install surface. Run `cargo mock refresh`");
    println!("  to pull the current canonical state. Anything mockspace-internal in");
    println!("  your repo's agent docs / hooks is overwritten by the refresh; only");
    println!("  consumer-authored conventions (project lints, repo-specific");
    println!("  workflows, the bits NOT about mockspace itself) are preserved.");
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

/// Run the lint engine against `repo_root` at the given `gate` and
/// `surface`. Composes the four engine pieces:
///
/// 1. `LintsConfig::load(repo_root, OverrideCascade::default())` reads
///    the user TOML + applies cascade (Layer 5 CLI overrides remain
///    empty for this slice; flagged for a follow-up).
/// 2. `MockspaceEngine::new()` instantiates the catalog-default lint
///    set.
/// 3. `engine.scope_project(repo_root, surface)` walks the project
///    tree and parses the relevant documents. The surface lets a
///    consumer simulate `ci` runs locally, or run as the `editor`
///    surface for LSP-style integrations.
/// 4. `engine.run(&project, gate, &cfg)` produces a `Vec<Finding>`.
///
/// Findings render one per line to stdout. Exit code is FAILURE iff
/// any finding's severity is `Error` at the chosen gate (matching
/// the pre-commit / pre-push hook gate semantics).
fn run_check(
    repo_root: &std::path::Path,
    gate: Gate,
    json: bool,
    surface: RunSurface,
) -> std::process::ExitCode {
    let cfg_obj = match LintsConfig::load(repo_root, OverrideCascade::default()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("check failed: could not load lints config: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // Per-lint cascade may have rejected entries (unknown TOML field,
    // invalid value, unknown finding kind, etc.). Each such failure
    // means a lint silently dropped from the active set; the user
    // would see "no findings" without ever knowing why. Surface every
    // ConfigError as a visible diagnostic and exit FAILURE so the
    // intended-versus-actual lint set never silently diverges.
    if !cfg_obj.config_errors.is_empty() {
        let fallback = repo_root
            .join("mock")
            .join("lints.toml")
            .display()
            .to_string();
        // Sort by lint name then field path so output is stable
        // across runs. Catalog iteration order is build-dependent;
        // golden tests cannot pin a build-dependent order.
        let mut sorted: Vec<&_> = cfg_obj.config_errors.iter().collect();
        sorted.sort_by(|a, b| {
            a.lint_name
                .cmp(&b.lint_name)
                .then_with(|| a.field_path.cmp(&b.field_path))
        });
        for ce in sorted {
            let (path, line, col) = match &ce.source_location {
                Some(span) => (
                    span.file.display().to_string(),
                    span.start_line,
                    span.start_column,
                ),
                None => (fallback.clone(), 0, 0),
            };
            eprintln!("{path}:{line}:{col}: [error] lint-config: {ce}");
        }
        eprintln!(
            "check failed: {n} lint configuration error(s); affected lints were dropped from the active set",
            n = cfg_obj.config_errors.len()
        );
        return std::process::ExitCode::FAILURE;
    }
    // The cascade-resolved entries carry the merged severities and
    // configs; the engine reads them at instantiation time. The
    // `cfg` argument to `engine.run` covers per-lint runtime
    // overrides, which the CLI does not surface yet.
    let engine = MockspaceEngine::with_entries(cfg_obj.entries);
    let project = match engine.scope_project(repo_root, surface) {
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

    // JSON branch: pretty-print the findings as a serde_json
    // array and short-circuit. Exit code semantics still hinge on
    // whether any Error-severity finding hit the gate; compute
    // that from `findings` before returning. The serde-derived
    // rendering of `Severity` / `Gate` uses the substrate's
    // canonical lowercase labels (`warn`, not `warning`), which
    // intentionally diverges from the human stdout vocabulary.
    if json {
        let body = match serde_json::to_string_pretty(&findings) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("check failed: could not serialise findings as JSON: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };
        println!("{body}");
        let had_error = findings.iter().any(|f| matches!(f.severity, Severity::Error));
        return if had_error {
            std::process::ExitCode::FAILURE
        } else {
            std::process::ExitCode::SUCCESS
        };
    }

    // Render the gate as a lowercase word that matches the
    // `--gate {commit|build|push}` flag value the user types. The
    // Debug derive on `Gate` produces PascalCase, which mismatches
    // the input vocabulary; a local match keeps the fix scoped to
    // the CLI without broadening `Gate`'s public surface.
    let gate_name = match gate {
        Gate::Commit => "commit",
        Gate::Build => "build",
        Gate::Push => "push",
    };
    if findings.is_empty() {
        println!("no findings at gate {gate_name}");
        return std::process::ExitCode::SUCCESS;
    }

    let mut had_error = false;
    for f in &findings {
        let path = f.span.file.display();
        let line = f.span.start_line;
        let col = f.span.start_column;
        // Render severity as the lowercase rustc-conventional label
        // (`error` / `warning` / `note` etc) rather than the Debug
        // PascalCase variant name. Same rationale as the gate
        // rendering: it's a CLI display preference, not a Severity
        // identity property; keep the fix scoped to the CLI.
        let sev = match f.severity {
            Severity::Skip => "skip",
            Severity::Off => "off",
            Severity::Hint => "hint",
            Severity::Info => "info",
            Severity::Warn => "warning",
            Severity::Error => "error",
        };
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
