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
    apply_plan, bootstrap,
    config_loader::{find_and_read_lints_toml, LintsConfig, LintsTomlFile, OverrideCascade},
    design_rounds::discover_design_rounds,
    engine::MockspaceEngine,
    explain, plan_fixes, preset_source, render_check, render_regenerate, render_unified_diff,
    scope_walk, AdvanceError, AdvanceReport, AdvanceVerb, ArchiveError, ArchiveReport, CheckReport,
    CloseMetadata, DesignRound, Finding, FixOpts, FlockTransitionLock, Gate, LintCfgStore,
    LintEngine, LockError, Namespace, ObjectId, Phase, RegenerateError, RegenerateReport,
    RepoError, RepoHandle, ReplanMode, RoundState, RunSurface, Severity, DefaultSlug, DefaultTaskId,
    TaskMeta, TaskResolution, WriteState,
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
        /// Lint name to explain. Lint names follow [`DefaultSlug`] shape
        /// (kebab-case, ASCII lowercase start). Misspellings that
        /// pass slug validation but do not match a catalog entry
        /// surface `LintNotFound` at lookup time.
        #[arg(value_parser = parse_slug)]
        name: DefaultSlug,
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
        /// Apply suggested fixes inline to source files. Findings
        /// without a `suggestion.fix` are skipped (advisory only).
        /// Findings still print to stdout (or JSON) before the
        /// fixes apply, so the user sees what was caught. Exit
        /// code still reflects the gate evaluation; `--fix` does
        /// not silence error-level findings, it just additionally
        /// applies their fixes when the lint provides one.
        #[arg(long)]
        fix: bool,
        /// With `--fix`, print the unified diff that would result
        /// instead of writing the changes. Implies `--fix`. Useful
        /// for previewing fixes in CI or for human review before
        /// committing the changes.
        #[arg(long)]
        dry_run: bool,
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
        #[arg(value_parser = parse_slug)]
        slug: DefaultSlug,
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
    /// Manage tasks. Slice A ships `new`, `list`, `show`; lifecycle
    /// verbs (start/block/defer/close), move semantics, archival,
    /// and step tracking land in follow-up slices per spec
    /// §16, §26.
    Task {
        #[command(subcommand)]
        verb: TaskVerb,
    },
    /// Render the `mock/*.md.tmpl` templates into `docs/`. Walks
    /// both the mock-root templates (DESIGN / PRINCIPLES / WORKFLOW)
    /// and every per-crate template under `mock/crates/<name>/`
    /// (DESIGN, BACKLOG, and any `deepdives/*.md.tmpl`). Per-crate
    /// `SHAME.md.tmpl` is intentionally never rendered.
    ///
    /// With `--check`, prints a drift report and exits non-zero
    /// when any rendered output diverges from disk or is missing.
    /// CI consumers use `--check`; user-driven regen uses the
    /// default write mode.
    Regenerate {
        /// Compare rendered output against `docs/` on disk and
        /// exit non-zero on any drift instead of writing.
        #[arg(long)]
        check: bool,
        /// Override the output directory. Defaults to
        /// `<repo-root>/docs`. Useful for dry-runs into a scratch
        /// location.
        #[arg(long, value_name = "PATH")]
        out_dir: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum PhaseVerb {
    /// Open a planning surface on a TOPIC-phase round. Rewrites
    /// `.phase` to `plan_doc`; the manifest authoring itself
    /// happens via separate ref edits.
    Plan {
        /// Slug of the round to open PLAN(doc) on. Must currently
        /// be in TOPIC phase.
        #[arg(value_parser = parse_slug)]
        slug: DefaultSlug,
    },
    /// Seal the authoring manifest and transition PLAN(side) ->
    /// APPLY(side). Requires the source-side branch tip OID as
    /// a hex SHA so the anchor capture sees a stable input.
    Apply {
        /// Slug of the round to seal. Must currently be in
        /// PLAN(doc) or PLAN(src) phase.
        #[arg(value_parser = parse_slug)]
        slug: DefaultSlug,
        /// Source-side branch tip OID at APPLY entry, in hex
        /// form (e.g. the output of `git rev-parse HEAD`). The
        /// anchor records this OID for provenance.
        #[arg(long, value_parser = parse_object_id)]
        source_tip: ObjectId,
    },
    /// Advance the round past APPLY: APPLY(doc) -> PLAN(src), or
    /// APPLY(src) -> DONE. The doc-side locked manifest is
    /// preserved through the doc-to-src transition.
    Finish {
        /// Slug of the round to advance. Must currently be in
        /// APPLY(doc) or APPLY(src) phase.
        #[arg(value_parser = parse_slug)]
        slug: DefaultSlug,
    },
    /// Deprecate the locked manifest and return APPLY(side) to
    /// PLAN(side). The locked manifest is renamed to the next
    /// `manifest.<side>.deprecated.<n>.toml` slot.
    Replan {
        /// Slug of the round to replan. Must currently be in
        /// APPLY(doc) or APPLY(src) phase.
        #[arg(value_parser = parse_slug)]
        slug: DefaultSlug,
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

#[derive(Subcommand, Debug)]
enum TaskVerb {
    /// Create a new task at `refs/mock/task/<ns-path>/<slug>`. The
    /// task starts in the `open` state with the supplied title. The
    /// task ID is parsed from URI form (`<seg>::<seg>::...::<slug>`);
    /// a single segment yields a top-level (no-namespace) task.
    New {
        /// Task identifier in URI form. Examples:
        /// `migrate-to-codeberg`, `compiler::ir::lower-pass::define-grammar`.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
        /// One-line human-facing title. Defaults to the slug if absent.
        /// Free-form prose; String is the right type here.
        #[arg(long)]
        title: Option<String>,
    },
    /// Enumerate every task ref under `refs/mock/task/*` and print
    /// the task identifiers in lexicographic order. Excludes the
    /// `refs/mock/task-archive` ref.
    List,
    /// Read a task ref's `meta.toml` and print its contents. Output
    /// is the canonical TOML representation; pair with a TOML parser
    /// to consume programmatically.
    Show {
        /// Task identifier in URI form.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
    },
    /// Transition a task into the `in-progress` state. Valid from any
    /// non-terminal (non-closed) state.
    Start {
        /// Task identifier in URI form.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
    },
    /// Transition a task into the `blocked` state.
    Block {
        /// Task identifier in URI form.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
    },
    /// Transition a task into the `deferred` state.
    Defer {
        /// Task identifier in URI form.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
    },
    /// Close a task. Writes the `[closure]` block into `meta.toml`
    /// and rotates the state marker to `closed`. Closed is terminal:
    /// subsequent lifecycle verbs refuse with a `Terminal` error.
    /// The closing branch / phase / round-slug arguments are
    /// optional; set them when the close is driven from inside an
    /// active round so the audit trail captures provenance.
    Close {
        /// Task identifier in URI form.
        #[arg(value_parser = parse_task_id)]
        id: DefaultTaskId,
        /// Why the task closed.
        #[arg(long, value_enum)]
        resolution: TaskResolutionArg,
        /// Source-side branch carrying the closing work. Free-form
        /// git branch name; no `BranchName` newtype exists yet, so
        /// `String` is documented as the boundary type here per
        /// `harness-the-type-system.md`'s "documented exceptions"
        /// clause. Track via #595 if a newtype lands later.
        #[arg(long)]
        branch: Option<String>,
        /// Phase marker at close time (e.g. `apply_src`).
        #[arg(long, value_parser = parse_phase)]
        phase: Option<Phase>,
        /// Round slug that closed this task.
        #[arg(long = "round-slug", value_parser = parse_slug)]
        round_slug: Option<DefaultSlug>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum TaskResolutionArg {
    /// Work shipped.
    Completed,
    /// Cancelled before ship.
    Cancelled,
    /// Replaced by another task.
    Superseded,
    /// Not going to be done by design choice.
    Wontfix,
}

impl From<TaskResolutionArg> for TaskResolution {
    fn from(value: TaskResolutionArg) -> Self {
        match value {
            TaskResolutionArg::Completed => TaskResolution::Completed,
            TaskResolutionArg::Cancelled => TaskResolution::Cancelled,
            TaskResolutionArg::Superseded => TaskResolution::Superseded,
            TaskResolutionArg::Wontfix => TaskResolution::Wontfix,
        }
    }
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

/// Cold-start helper for subcommands that benefit from having the
/// builtin agent rules extracted to `<root>/mock/target/agent/`.
/// Calls `bootstrap::ensure_agent_extracted`, which is a no-op when
/// the extract is current. Runs silently on success (presence is
/// reported by `cargo mock status`, not by every other subcommand).
/// On error, logs a warning but does not abort the subcommand: the
/// agent rules are a nice-to-have for the consumer's agent reading
/// them, not a runtime dependency of the subcommand itself.
///
/// Not called from `status` (observational; should have no side
/// effects), nor from `install` / `uninstall` / `refresh` (those
/// drive agent extraction or removal themselves).
fn ensure_agent_ready(repo_root: &std::path::Path) {
    // The successful `InstallOutcome` (Installed vs AlreadyInstalled)
    // is dropped on purpose. Reporting cold-extract events here would
    // appear on stderr of every subcommand and broke golden tests on
    // the chatty first iteration. Slice 4's `cargo mock status`
    // surface reads the same sentinel state from disk and is the
    // right place to surface presence / staleness to the consumer.
    if let Err(e) = bootstrap::ensure_agent_extracted(repo_root) {
        eprintln!(
            "warning: could not extract mockspace agent rules: {e}. Subcommand continues without them."
        );
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
        Command::Explain { name } => {
            ensure_agent_ready(&repo_root);
            run_explain(&repo_root, &name)
        }
        Command::Check { gate, json, surface, fix, dry_run } => {
            ensure_agent_ready(&repo_root);
            run_check(&repo_root, gate.into(), json, surface.into(), fix, dry_run)
        }
        Command::Phase { verb } => {
            ensure_agent_ready(&repo_root);
            run_phase(&repo_root, verb)
        }
        Command::Close { slug } => {
            ensure_agent_ready(&repo_root);
            run_close(&repo_root, slug)
        }
        Command::Migrate => {
            ensure_agent_ready(&repo_root);
            run_migrate(&repo_root)
        }
        Command::Regenerate { check, out_dir } => {
            ensure_agent_ready(&repo_root);
            let out = out_dir.unwrap_or_else(|| repo_root.join("docs"));
            run_regenerate(&repo_root, &out, check)
        }
        Command::Task { verb } => {
            ensure_agent_ready(&repo_root);
            run_task(&repo_root, verb)
        }
    }
}

/// Parse a slug string into [`DefaultSlug`] with a CLI-friendly error
/// message. The `DefaultSlug::new` validation rejects empty, too-long,
/// and out-of-charset inputs.
///
/// Used as a clap `value_parser` so wrong-shape inputs fail at
/// argument-parse time rather than inside a runner.
fn parse_slug(raw: &str) -> Result<DefaultSlug, String> {
    DefaultSlug::new(raw).map_err(|e| format!("invalid slug `{raw}`: {e}"))
}

/// Parse a task identifier (URI form `<seg>::<seg>::...::<slug>`)
/// into [`DefaultTaskId`] with a CLI-friendly error message. Used as a
/// clap `value_parser`.
fn parse_task_id(raw: &str) -> Result<DefaultTaskId, String> {
    DefaultTaskId::parse(raw).map_err(|e| format!("invalid task id `{raw}`: {e}"))
}

/// Parse a hex object id into [`ObjectId`] with a CLI-friendly
/// error message. Used as a clap `value_parser` for the
/// `--source-tip <hex>` flag.
fn parse_object_id(raw: &str) -> Result<ObjectId, String> {
    ObjectId::from_hex(raw.as_bytes())
        .map_err(|e| format!("invalid object id `{raw}`: {e}"))
}

/// Parse a phase marker (e.g. `apply_src`, `plan_doc`) into
/// [`Phase`]. Used as a clap `value_parser` for flags that name a
/// phase by its on-disk marker.
fn parse_phase(raw: &str) -> Result<Phase, String> {
    Phase::from_marker(raw)
        .ok_or_else(|| format!("unknown phase marker `{raw}`"))
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
    // clap's `value_parser`s have already validated the per-verb
    // inputs (slug shape, source-tip hex). The body just maps the
    // typed PhaseVerb into the typed AdvanceVerb the executor takes.
    let (slug, advance_verb) = match verb {
        PhaseVerb::Plan { slug } => (slug, AdvanceVerb::Plan),
        PhaseVerb::Apply { slug, source_tip } => (
            slug,
            AdvanceVerb::Apply {
                source_branch_tip: source_tip,
            },
        ),
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
            print_advance_report(&slug, &report);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("phase transition failed: {}", render_advance_error(&e));
            std::process::ExitCode::FAILURE
        }
    }
}

fn print_advance_report(slug: &DefaultSlug, report: &AdvanceReport) {
    println!(
        "round `{slug}` -> phase `{phase}` via verb `{verb:?}`",
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

fn run_task(repo_root: &std::path::Path, verb: TaskVerb) -> std::process::ExitCode {
    let handle = match open_repo(repo_root) {
        Ok(h) => h,
        Err(msg) => {
            eprintln!("{msg}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match verb {
        TaskVerb::New { id, title } => run_task_new(&handle, id, title.as_deref()),
        TaskVerb::List => run_task_list(&handle),
        TaskVerb::Show { id } => run_task_show(&handle, &id),
        TaskVerb::Start { id } => run_task_transition(&handle, &id, TaskTransitionVerb::Start),
        TaskVerb::Block { id } => run_task_transition(&handle, &id, TaskTransitionVerb::Block),
        TaskVerb::Defer { id } => run_task_transition(&handle, &id, TaskTransitionVerb::Defer),
        TaskVerb::Close {
            id,
            resolution,
            branch,
            phase,
            round_slug,
        } => run_task_close(&handle, &id, resolution, branch.as_deref(), phase, round_slug.as_ref()),
    }
}

/// Internal tag selecting which non-close lifecycle verb to dispatch.
/// Close has its own runner because it carries extra arguments.
#[derive(Clone, Copy)]
enum TaskTransitionVerb {
    Start,
    Block,
    Defer,
}

fn run_task_new(
    handle: &RepoHandle,
    task_id: DefaultTaskId,
    title: Option<&str>,
) -> std::process::ExitCode {
    let namespace_path = task_id
        .namespace()
        .map(|ns| ns.as_ref_path())
        .unwrap_or_default();
    let title = title.unwrap_or_else(|| task_id.slug().as_str()).to_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let meta = TaskMeta {
        // TaskMeta's serde-derived String fields are the wire format
        // (TOML); the in-memory shape should retype to DefaultSlug/DefaultNamespace
        // via #595's serde adapter pattern. Until that lands, the
        // CLI does the bottom-of-the-boundary conversion here.
        mockspace_version: env!("CARGO_PKG_VERSION").to_owned(),
        slug: task_id.slug().as_str().to_owned(),
        namespace: namespace_path,
        title,
        // ISO-8601 from unix epoch. minimal stdlib-only formatter
        // suitable for the synthetic mockspace identity.
        created: format_iso8601(now),
        priority: None,
        group: None,
        steps: Default::default(),
        refs: Default::default(),
        closure: None,
    };
    match handle.create_task(&task_id, &meta) {
        Ok(report) => {
            println!("task `{}` created", task_id.as_uri_form());
            println!("  ref: {}", report.ref_path);
            println!("  commit: {}", report.commit_oid);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("task new failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_task_list(handle: &RepoHandle) -> std::process::ExitCode {
    match handle.list_tasks() {
        Ok(tasks) => {
            if tasks.is_empty() {
                println!("no tasks");
            } else {
                for t in tasks {
                    println!("{}", t.as_uri_form());
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("task list failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_task_show(handle: &RepoHandle, task_id: &DefaultTaskId) -> std::process::ExitCode {
    match handle.show_task(task_id) {
        Ok(meta) => match meta.to_toml() {
            Ok(toml) => {
                // toml::to_string_pretty does not guarantee a
                // trailing newline; use println! so the rendered
                // body terminates cleanly under shell consumers.
                println!("{}", toml.trim_end_matches('\n'));
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("task show: failed to serialise meta as TOML: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("task show failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_task_transition(
    handle: &RepoHandle,
    task_id: &DefaultTaskId,
    verb: TaskTransitionVerb,
) -> std::process::ExitCode {
    let (verb_name, result) = match verb {
        TaskTransitionVerb::Start => ("start", handle.start_task(task_id)),
        TaskTransitionVerb::Block => ("block", handle.block_task(task_id)),
        TaskTransitionVerb::Defer => ("defer", handle.defer_task(task_id)),
    };
    match result {
        Ok(report) => {
            println!(
                "task `{}` transitioned: {} -> {}",
                task_id.as_uri_form(),
                report.previous_state,
                report.new_state,
            );
            println!("  ref: {}", report.ref_path);
            println!("  commit: {}", report.commit_oid);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("task {verb_name} failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_task_close(
    handle: &RepoHandle,
    task_id: &DefaultTaskId,
    resolution: TaskResolutionArg,
    branch: Option<&str>,
    phase: Option<Phase>,
    round_slug: Option<&DefaultSlug>,
) -> std::process::ExitCode {
    // CloseMetadata still carries String fields per #595's pending
    // retype. Collapse typed inputs to their wire form here at the
    // CLI/IO boundary; the typed values survive up to this point.
    let metadata = CloseMetadata {
        resolution: resolution.into(),
        closed_branch: branch.unwrap_or("").to_owned(),
        closing_phase: phase.map(|p| phase_marker(p).to_owned()).unwrap_or_default(),
        closing_round_slug: round_slug
            .map(|s| s.as_str().to_owned())
            .unwrap_or_default(),
    };
    match handle.close_task(task_id, metadata) {
        Ok(report) => {
            println!(
                "task `{}` closed: {} -> {}",
                task_id.as_uri_form(),
                report.previous_state,
                report.new_state,
            );
            println!("  ref: {}", report.ref_path);
            println!("  commit: {}", report.commit_oid);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("task close failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Format a unix epoch as a minimal ISO-8601 UTC string. Avoids
/// pulling chrono/time crates for a single timestamp surface; the
/// format is sufficient for the synthetic mockspace authoring
/// identity, which carries the same shape as the round refs'
/// committer timestamps.
fn format_iso8601(unix_secs: u64) -> String {
    // Compute Y/M/D from epoch using the algorithm from RFC 3339
    // appendix A. Adapted to UTC, no leap-seconds.
    let days_since_epoch = (unix_secs / 86_400) as i64;
    let seconds_of_day = (unix_secs % 86_400) as u32;
    let hh = seconds_of_day / 3600;
    let mm = (seconds_of_day % 3600) / 60;
    let ss = seconds_of_day % 60;
    // Convert days-since-1970-01-01 to civil date using Howard
    // Hinnant's days_from_civil inverse.
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"
    )
}

fn run_close(repo_root: &std::path::Path, slug: DefaultSlug) -> std::process::ExitCode {
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
            print_archive_report(&slug, &report);
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("archive failed: {}", render_archive_error(&e));
            std::process::ExitCode::FAILURE
        }
    }
}

fn print_archive_report(slug: &DefaultSlug, report: &ArchiveReport) {
    println!("round `{slug}` archived to refs/mock/round-archive");
    println!("  new archive commit: {}", report.archive_commit);
    println!("  entries archived: {}", report.entries_archived);
    if report.source_ref_deleted {
        println!("  source ref `refs/mock/round/{slug}` deleted");
    } else {
        println!(
            "  WARNING: source ref `refs/mock/round/{slug}` NOT deleted; re-run is idempotent"
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
    println!("To fix by hand:");
    println!();
    println!("- CI workflows calling the old verbs:");
    println!("    lock      -> cargo mock phase apply <slug> --source-tip <hex>");
    println!("    deprecate -> cargo mock phase replan <slug> [--mode ...]");
    println!("    close     -> (unchanged)");
    println!();
    println!("- Tasks, memos, prose that mention `mock lock` or `mock deprecate`.");
    println!("  Grep and update.");
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
    println!("  builtin agent rules   : {}", s.agent_extract.label());
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
fn run_explain(repo_root: &std::path::Path, lint_name: &DefaultSlug) -> std::process::ExitCode {
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
    // explain_lint's mockspace-rs signature takes &str today; that's
    // a #594-tracked surface to retype. Passing through the slug's
    // validated str preserves type-safety up to the boundary.
    match explain::explain_lint(lint_name.as_str(), &user_toml, &overrides, &source) {
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
    fix: bool,
    dry_run: bool,
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
    let mut findings = match engine.run(&project, gate, &cfg_store) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("check failed: engine dispatch error: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // Stable sort by (file, line, column, severity). The engine
    // returns findings in catalog-dispatch order, which is
    // deterministic per build but changes whenever catalog
    // registration reorders. A presentation-side sort gives
    // consumers a stable diagnostic ordering independent of
    // catalog churn. `sort_by_key` keeps the existing within-tie
    // order so multiple findings on the same span at the same
    // severity preserve dispatch-relative order. The engine API
    // contract stays "dispatch order"; this is a CLI render
    // affordance for the human + JSON output paths.
    findings.sort_by_key(|f| {
        (
            f.span.file.clone(),
            f.span.start_line,
            f.span.start_column,
            f.severity,
        )
    });

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
        if fix || dry_run {
            if let Some(code) = apply_fixes(repo_root, &findings, dry_run) {
                return code;
            }
        }
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
    if fix || dry_run {
        if let Some(code) = apply_fixes(repo_root, &findings, dry_run) {
            return code;
        }
    }
    if had_error {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// Apply (or preview) auto-fixes for any finding carrying a
/// `suggestion.fix`. Returns `Some(exit)` when the fix path
/// short-circuits with its own exit code (planning error, IO error
/// during apply); returns `None` to let the caller's existing
/// gate-evaluation logic decide the final exit code.
///
/// When `dry_run` is true, prints the unified diff that would result
/// and never writes; otherwise calls `apply_plan` and prints a tally.
fn apply_fixes(
    repo_root: &std::path::Path,
    findings: &[Finding],
    dry_run: bool,
) -> Option<std::process::ExitCode> {
    let opts = FixOpts { dry_run, only_lints: None };
    let plan = match plan_fixes(repo_root, findings, &opts) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("fix planning failed: {e}");
            return Some(std::process::ExitCode::FAILURE);
        }
    };

    if dry_run {
        let diff = render_unified_diff(&plan);
        if diff.is_empty() {
            println!("no fixable findings");
        } else {
            print!("{diff}");
        }
        return None;
    }

    match apply_plan(&plan, &opts, repo_root) {
        Ok(()) => {
            println!(
                "applied {applied} fix(es); {conflicts} conflict(s), {skipped} advisory finding(s) skipped",
                applied = plan.fixes_applied,
                conflicts = plan.conflicts.len(),
                skipped = plan.skipped_advisory,
            );
            None
        }
        Err(e) => {
            eprintln!("fix application failed: {e}");
            Some(std::process::ExitCode::FAILURE)
        }
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// Render the `mock/*.md.tmpl` templates into `out_dir`, or compare
/// against on-disk output and exit non-zero on drift when `check` is
/// set.
///
/// Walks the project via `scope_walk` to assemble the `MockspaceProject`
/// the render module needs (crate graph, workspace members, design
/// rounds, etc.). The walk reuses the same scope inference the lint
/// engine uses, so behaviour stays consistent across CLI surfaces.
fn run_regenerate(
    repo_root: &std::path::Path,
    out_dir: &std::path::Path,
    check_only: bool,
) -> std::process::ExitCode {
    let project = match scope_walk(repo_root, RunSurface::Local) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "regenerate failed: could not walk project at `{}`: {e}",
                repo_root.display()
            );
            return std::process::ExitCode::FAILURE;
        }
    };

    if check_only {
        match render_check(&project, out_dir) {
            Ok(report) => print_check_report(&report, out_dir),
            Err(e) => {
                eprintln!("regenerate --check failed: {}", format_render_error(&e));
                return std::process::ExitCode::FAILURE;
            }
        }
    } else {
        match render_regenerate(&project, out_dir) {
            Ok(report) => print_regenerate_report(&report, out_dir),
            Err(e) => {
                eprintln!("regenerate failed: {}", format_render_error(&e));
                return std::process::ExitCode::FAILURE;
            }
        }
    }
}

fn print_regenerate_report(
    report: &RegenerateReport,
    out_dir: &std::path::Path,
) -> std::process::ExitCode {
    let (mut created, mut updated, mut unchanged) = (0usize, 0usize, 0usize);
    for f in &report.files {
        match f.state {
            WriteState::Created => created += 1,
            WriteState::Updated => updated += 1,
            WriteState::Unchanged => unchanged += 1,
        }
    }
    println!(
        "rendered {} files into `{}`: {} created, {} updated, {} unchanged",
        report.files.len(),
        out_dir.display(),
        created,
        updated,
        unchanged,
    );
    std::process::ExitCode::SUCCESS
}

fn print_check_report(
    report: &CheckReport,
    out_dir: &std::path::Path,
) -> std::process::ExitCode {
    if report.needs_regen() {
        println!(
            "drift detected against `{}`: {} drifted, {} missing, {} matched",
            out_dir.display(),
            report.drifted.len(),
            report.missing.len(),
            report.matched.len(),
        );
        for path in &report.drifted {
            println!("  drifted: {}", path.display());
        }
        for path in &report.missing {
            println!("  missing: {}", path.display());
        }
        std::process::ExitCode::FAILURE
    } else {
        println!(
            "no drift against `{}`: {} matched",
            out_dir.display(),
            report.matched.len(),
        );
        std::process::ExitCode::SUCCESS
    }
}

fn format_render_error(e: &RegenerateError) -> String {
    match e {
        RegenerateError::Io { path, source } => {
            format!("io error on `{}`: {source}", path.display())
        }
        RegenerateError::TemplateMissing(p) => {
            format!("template missing: `{}`", p.display())
        }
        RegenerateError::Render(r) => format!("template render error: {r}"),
    }
}
