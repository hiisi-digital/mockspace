//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Best-effort `cargo-deny` gate for pre-push.
//!
//! Runs `cargo deny check` in each workspace root the repo contains, each
//! pointing at the repo's single `deny.toml` via `--config`, so one config
//! governs every root (including a nested `mock/` workspace and any excluded
//! sub-workspace). This is what catches license-incompatible or advisory-flagged
//! transitive dependencies across the whole graph, not only the root workspace's.
//!
//! Blocks the push when cargo-deny reports a violation in some workspace root.
//! Skipped (never blocking), each with its own reason: no `deny.toml` at the
//! repo root; cargo-deny not installed; a workspace root whose relative path
//! carries a whole `research` path component (a spike tree, which also covers
//! `mock/research/sketches/**`), whatever `cargo metadata` or `cargo deny`
//! itself say about it; or a root whose `cargo metadata` resolves to an empty
//! package list. A non-spike root whose package graph cannot be read at all
//! blocks rather than being skipped: `cargo metadata` failing outright says
//! nothing about whether the manifest is clean. Opt out with
//! `deny_check = false` in `mockspace.toml`.
//!
//! A pass shows one line per root and none of what cargo-deny printed; a failure
//! shows only its errors, its summary line and its exit status
//! ([`failure_text`]). The gate also stands down for a push whose range the hook
//! claims changes no `Cargo.toml`, `Cargo.lock`, `deny.toml` or `.cargo/config*`
//! ([`Claim`], [`gate`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run the deny gate. `Err(message)` on either of two things: cargo-deny
/// reporting a violation in some workspace root, or a non-spike root whose
/// package graph could not be read at all (the pre-push gate blocks on both).
/// `Ok(actions)` otherwise, including every skip path, with the action lines
/// for the caller to log:
///
/// - the repo has no `deny.toml` (nothing to run against);
/// - cargo-deny is not installed;
/// - a workspace root is a spike tree, per [`is_a_spike_tree`];
/// - a workspace root's package graph resolved to no packages at all
///   (`PackageGraph::Empty`);
/// - a workspace root's `cargo deny check` passed cleanly.
pub fn check(repo_root: &Path, enabled: bool) -> Result<Vec<String>, String> {
    let mut actions = Vec::new();
    if !enabled {
        return Ok(actions);
    }
    let config = repo_root.join("deny.toml");
    if !config.is_file() {
        return Ok(actions);
    }
    // Absolute config path so `--config` stays valid under the per-root
    // `current_dir` we set for each `cargo deny` invocation. `is_file`
    // passed, so canonicalize resolves; keep the joined path if it somehow
    // does not (a relative repo_root would then be the footgun, not this).
    let config = config.canonicalize().unwrap_or(config);
    if !cargo_deny_installed() {
        actions.push(
            "deny_check: skipped, cargo-deny not installed (cargo install cargo-deny)".to_string(),
        );
        return Ok(actions);
    }

    for root in workspace_roots(repo_root) {
        let where_ = rel(repo_root, &root);
        let spike = is_a_spike_tree(&under(repo_root, &root));
        // Spike roots are decided before any cargo call is made for them: a
        // probe or a sketch takes shortcuts everywhere and nothing it depends
        // on reaches a consumer, so nothing `cargo metadata` or `cargo deny`
        // could say about its graph changes the answer. `decide_root_action`
        // still takes a `PackageGraph`, for a caller that has one; this one
        // is a placeholder the function never inspects once `spike` is true,
        // which is what the exhaustive unit tests below establish.
        let graph = if spike {
            PackageGraph::Present
        } else {
            // A workspace with no packages has no dependency graph, so
            // cargo-deny exits non-zero on `cargo metadata` before it looks
            // at a single licence. Reading that as a violation blocks a push
            // over a repository layout somebody chose on purpose, and says
            // the wrong thing about why; `decide_root_action` treats it as
            // its own skip rather than folding it into `Unreadable`.
            package_graph(&root)
        };
        match decide_root_action(spike, graph) {
            RootAction::Skip(SkipReason::SpikeTree) => {
                actions.push(format!("deny_check: skipped, {where_} is a spike tree"));
            },
            RootAction::Skip(SkipReason::EmptyGraph) => {
                actions.push(format!(
                    "deny_check: skipped, no package graph to check ({where_})"
                ));
            },
            RootAction::Block => {
                return Err(format!(
                    "deny_check: could not read the package graph in {where_}, so nothing was checked"
                ));
            },
            RootAction::RunDeny => {
                match run_deny(&root, &config) {
                    Ok(()) => {
                        actions.push(format!("deny_check: cargo deny check passed ({where_})"));
                    },
                    Err(why) => {
                        return Err(format!(
                            "cargo deny check failed in {where_} (license/advisory/ban/source violation)\n{why}"
                        ));
                    },
                }
            },
        }
    }
    Ok(actions)
}

/// What to do with one workspace root, given whether it is a spike tree and
/// what `cargo metadata` said about its package graph. Pure: no process is
/// spawned here, which is what lets every branch be unit-tested without
/// `cargo` or `cargo-deny` installed.
///
/// `spike` decides the outcome outright, regardless of `graph`: a spike root
/// is always [`SkipReason::SpikeTree`], whatever state its graph is in.
/// `check()` relies on that to skip calling `cargo metadata` for a spike root
/// at all, by never computing a real `graph` for one; the tests below drive
/// every `PackageGraph` variant through `spike = true` to establish the same
/// thing from the pure side.
#[derive(Debug, PartialEq, Eq)]
enum RootAction {
    /// Never blocks. Carries the reason for the caller's action line.
    Skip(SkipReason),
    /// The graph could not be read and the root is not a spike tree: blocks.
    Block,
    /// Neither a skip nor a block: run `cargo deny check` and act on it.
    RunDeny,
}

#[derive(Debug, PartialEq, Eq)]
enum SkipReason {
    /// The root sits inside the audit trail, per [`is_a_spike_tree`].
    SpikeTree,
    /// `cargo metadata` resolved to no packages at all.
    EmptyGraph,
}

fn decide_root_action(spike: bool, graph: PackageGraph) -> RootAction {
    if spike {
        return RootAction::Skip(SkipReason::SpikeTree);
    }
    match graph {
        PackageGraph::Empty => RootAction::Skip(SkipReason::EmptyGraph),
        PackageGraph::Unreadable => RootAction::Block,
        PackageGraph::Present => RootAction::RunDeny,
    }
}

/// Whether a root sits inside the audit trail rather than the shipped tree.
///
/// Matches exactly one thing: a whole `research` path component, so a crate
/// named `research-tools` is not caught by a crate named `research`. That is
/// the workspace's own reserved name for a panel's probes, `mock/research/**`,
/// and it is also what a feasibility sketch lives under here: a sketch is
/// `mock/research/sketches/**`, a `research` component away from anywhere
/// else, so a bare `sketches` component with no `research` above it, such as
/// `tools/sketches/x`, is not a spike tree and blocks like any other root.
/// Matching `sketches` on its own used to be part of this, back when the
/// spike exemption fired only where `cargo metadata` had already failed; now
/// that it is unconditional, a name that wide would skip a legitimately
/// shipped `sketches` crate outside `research/`.
///
/// A first-party crate that genuinely lives under a `research` component
/// would be skipped rather than blocked, whatever `cargo metadata` or `cargo
/// deny` itself say about it. That is the wrong answer in principle and a
/// tolerable one in practice, because the action line names the root it
/// skipped and why, so the skip is readable rather than silent.
///
/// Takes a path rather than the rendered message string, so what counts as a
/// component is the platform's answer and not whichever separator `display`
/// happened to use.
fn is_a_spike_tree(rel_path: &Path) -> bool {
    rel_path.components().any(|c| c.as_os_str() == "research")
}

/// The part of `path` below `repo_root`, for deciding things about where a root
/// sits. Falls back to `.`, which is not a spike tree, so a path that somehow
/// does not sit under the repo blocks rather than being skipped on the strength
/// of a component from somewhere above the repository.
fn under(repo_root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(repo_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// What `cargo metadata` says about the packages at one workspace root.
///
/// Three states, and the third is why this is not a bool. A graph that cannot
/// be read is not an absent graph: skipping on it would let a repository whose
/// manifest is broken push with nothing checked, and log that there was nothing
/// to check.
enum PackageGraph {
    /// Packages resolved. cargo-deny has something to analyse.
    Present,
    /// Resolved to nothing. A virtual manifest with no members is the shape:
    /// a repository whose crates each carry their own `[workspace]`, so none of
    /// them reaches a consumer's dependency graph, has exactly this at the top.
    Empty,
    /// cargo could not be run, exited non-zero, or answered with something this
    /// cannot read. Nothing is known either way.
    Unreadable,
}

fn package_graph(root: &Path) -> PackageGraph {
    // The exit code alone is not the signal: `cargo metadata` succeeds on an
    // empty workspace and reports an empty `packages` array. cargo-deny is the
    // one that then exits non-zero, saying the manifest contains no package.
    let Ok(out) = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return PackageGraph::Unreadable;
    };
    if !out.status.success() {
        return PackageGraph::Unreadable;
    }
    match serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()
        .and_then(|v| v.get("packages").and_then(|p| p.as_array()).map(Vec::len))
    {
        Some(0) => PackageGraph::Empty,
        Some(_) => PackageGraph::Present,
        None => PackageGraph::Unreadable,
    }
}

/// Whether the `cargo-deny` subcommand is available.
fn cargo_deny_installed() -> bool {
    Command::new("cargo")
        .args(["deny", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `cargo deny --color never --config <config> check` in `root`. `Ok` on a
/// clean check, and silent: a pass hands back nothing of what cargo-deny
/// printed, which on a graph with a few hundred crates is every duplicate, every
/// unused allowance and every inclusion graph it knows. `Err` carries what
/// explains a failure, per [`failure_text`].
fn run_deny(root: &Path, config: &Path) -> Result<(), String> {
    run_deny_with(OsStr::new("cargo"), root, config)
}

/// [`run_deny`] with the program named, which is what lets a test stand in for
/// `cargo` and print whatever it likes. `--color never` is what keeps the
/// headers [`failure_text`] reads plain text whatever `CARGO_TERM_COLOR` says,
/// and both streams are captured, since cargo-deny writes its diagnostics to
/// stderr and its closing summary line to stdout.
fn run_deny_with(program: &OsStr, root: &Path, config: &Path) -> Result<(), String> {
    let out = Command::new(program)
        .arg("deny")
        .arg("--color")
        .arg("never")
        .arg("--config")
        .arg(config)
        .arg("check")
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not run `cargo deny`: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(failure_text(
        &String::from_utf8_lossy(&out.stderr),
        &String::from_utf8_lossy(&out.stdout),
        out.status.code(),
    ))
}

/// How many trailing lines of the stream a failure with no `error` diagnostic in
/// it keeps.
const FAILURE_TAIL_LINES: usize = 40;

/// What of cargo-deny's output explains a failed check: from `stderr`, the
/// blocks that open with an `error` header, up to the next header of any
/// severity; from `stdout`, the closing `advisories ... sources ...` summary
/// line; and the exit status, which tells a failed check (4) from a configuration
/// cargo-deny could not read (1). The warnings are what fill the stream on a
/// passing run too and say nothing about why this one failed. A failure with no
/// `error` block in it, cargo-deny dying on something that is not a diagnostic,
/// keeps the last [`FAILURE_TAIL_LINES`] lines of `stderr` instead, since keeping
/// nothing would make it a failure that says nothing.
fn failure_text(stderr: &str, stdout: &str, code: Option<i32>) -> String {
    let mut kept = String::new();
    let mut keep = false;
    for line in stderr.lines() {
        if let Some(error) = diagnostic_header(line) {
            keep = error;
        }
        if keep {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    if kept.is_empty() {
        let lines: Vec<&str> = stderr.lines().collect();
        let from = lines.len().saturating_sub(FAILURE_TAIL_LINES);
        for line in &lines[from ..] {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    for line in stdout.lines().filter(|l| l.starts_with("advisories ")) {
        kept.push_str(line);
        kept.push('\n');
    }
    match code {
        Some(code) => kept.push_str(&format!("cargo deny exited with status {code}\n")),
        None => kept.push_str("cargo deny was ended by a signal\n"),
    }
    kept
}

/// Whether `line` opens a rendered diagnostic, and if it does whether that one
/// is an error. A header is flush left and reads `error[code]:`, `warning:`,
/// `note[code]:` or `help:`; the lines under it are indented or blank.
fn diagnostic_header(line: &str) -> Option<bool> {
    for severity in ["error", "warning", "note", "help"] {
        let Some(rest) = line.strip_prefix(severity) else {
            continue;
        };
        if rest.starts_with(':') || rest.starts_with('[') {
            return Some(severity == "error");
        }
    }
    None
}

/// What the caller says of the dependency inputs of a push. The generated
/// pre-push hook sends [`Claim::DepsUnchanged`] (`--deps-unchanged`): it takes each
/// pushed range's diff and sends it only when no `Cargo.toml`, `Cargo.lock`,
/// `deny.toml` or `.cargo/config*` anywhere in the repository is in it. The engine
/// does not see the range and takes the flag from any caller, hand-typed or not,
/// so this is the caller's claim and not something it checked; a hook generated
/// before the flag existed cannot send it, so a stale hook keeps the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// Nothing is said, which is every hand run and every older hook.
    None,
    /// The hook's claim that the pushed range changes no dependency input.
    DepsUnchanged,
}

/// Whether the pre-push dependency gate runs on this invocation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Not a push, or `deny_check = false`: nothing to say about it.
    Off,
    /// Run it.
    Run,
    /// A push whose range, by the hook's claim, cannot have changed what
    /// cargo-deny reads from the repository. What it reads from outside it, the
    /// advisory database and the yank state of a crate, moves with no push at all
    /// and is only seen by a push that runs the gate.
    StandDown,
}

/// Which of the three this invocation is. `push` is whether the run is the
/// pre-push one, `enabled` is `deny_check` from the config, and `claim` is what
/// the caller said of the pushed range's dependency inputs.
pub(crate) fn gate(push: bool, enabled: bool, claim: Claim) -> Gate {
    if !push || !enabled {
        Gate::Off
    } else if claim == Claim::DepsUnchanged {
        Gate::StandDown
    } else {
        Gate::Run
    }
}

/// Whether the repository has a `deny.toml` for the gate to run against, which is
/// what makes saying it stood down worth a line.
pub(crate) fn has_config(repo_root: &Path) -> bool {
    repo_root.join("deny.toml").is_file()
}

/// Every workspace root under `repo_root`: directories whose `Cargo.toml`
/// declares a `[workspace]` table. cargo-deny operates per workspace lockfile,
/// so each is checked independently against the shared config.
fn workspace_roots(repo_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    collect_workspace_roots(repo_root, 0, &mut roots);
    roots
}

/// Recursive helper for [`workspace_roots`], bounded in depth and skipping build
/// output and vcs/vendor directories that never hold a first-party workspace.
fn collect_workspace_roots(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    if is_workspace_manifest(&dir.join("Cargo.toml")) {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // `file_type` reads the dir entry directly and does NOT follow
        // symlinks, so a symlinked directory reports `is_dir() == false`
        // and is skipped: no symlink traversal, no cycle exposure.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let skip = matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("target" | ".git" | "node_modules" | ".cargo")
        );
        if !skip {
            collect_workspace_roots(&path, depth + 1, out);
        }
    }
}

/// Whether a `Cargo.toml` at `manifest` declares a `[workspace]` table.
fn is_workspace_manifest(manifest: &Path) -> bool {
    std::fs::read_to_string(manifest)
        .map(|s| s.lines().any(|l| l.trim_start().starts_with("[workspace]")))
        .unwrap_or(false)
}

/// Display `path` relative to `repo_root`, `.` for the root itself.
fn rel(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use super::*;

    /// The first line of every test that runs `cargo deny` for real: names the tool
    /// when it is missing, where `check()` would return `Ok` having done nothing and
    /// the test would fail on a message about something else.
    fn require_cargo_deny() {
        assert!(
            cargo_deny_installed(),
            "this test needs `cargo-deny` on PATH and it is not installed: \
             `cargo install cargo-deny --locked`"
        );
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn workspace_roots_finds_nested_and_skips_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // root workspace, a nested mock/ workspace, a plain member package, and a
        // stray workspace manifest under target/ that must be skipped.
        write(&root.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        write(&root.join("mock/Cargo.toml"), "[workspace]\nmembers = []\n");
        write(
            &root.join("mock/crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\n",
        );
        write(&root.join("target/junk/Cargo.toml"), "[workspace]\n");

        let found: BTreeSet<PathBuf> = workspace_roots(root).into_iter().collect();
        let expected: BTreeSet<PathBuf> = [root.to_path_buf(), root.join("mock")]
            .into_iter()
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn check_skips_when_no_config() {
        let tmp = tempfile::tempdir().unwrap();
        // no deny.toml at the root: check is a no-op, never blocks.
        let actions = check(tmp.path(), true).unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn check_disabled_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("deny.toml"), "[licenses]\n");
        // disabled: returns Ok with no actions even though a config exists.
        assert!(check(tmp.path(), false).unwrap().is_empty());
    }

    #[test]
    fn workspace_roots_skips_every_vcs_and_vendor_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        // a stray workspace manifest under each skipped dir must not surface.
        for skipped in ["target", ".git", "node_modules", ".cargo"] {
            write(
                &root.join(skipped).join("Cargo.toml"),
                "[workspace]\nmembers = []\n",
            );
        }
        let found: BTreeSet<PathBuf> = workspace_roots(root).into_iter().collect();
        assert_eq!(found, [root.to_path_buf()].into_iter().collect());
    }

    #[test]
    fn workspace_roots_respects_depth_bound() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        // a workspace manifest nested one level past the depth-8 cap is not found.
        let deep = root.join("a/b/c/d/e/f/g/h/i");
        write(&deep.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        let found: BTreeSet<PathBuf> = workspace_roots(root).into_iter().collect();
        assert!(!found.contains(&deep));
        assert!(found.contains(root));
    }

    #[test]
    fn rel_renders_root_as_dot() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(rel(root, root), ".");
        assert_eq!(rel(root, &root.join("mock")), "mock");
    }

    #[cfg(unix)]
    #[test]
    fn workspace_roots_does_not_traverse_symlinked_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        // a real workspace outside the tree, reachable only via a symlink.
        let outside = tmp.path().parent().unwrap().join("deny_symlink_target");
        write(&outside.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        let found: BTreeSet<PathBuf> = workspace_roots(root).into_iter().collect();
        // only the real root; the symlinked workspace is not traversed.
        assert_eq!(found, [root.to_path_buf()].into_iter().collect());
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn a_workspace_with_no_members_has_no_package_graph() {
        // The shape a repository has when every crate carries its own
        // `[workspace]` so none of them reaches a consumer's graph. cargo-deny
        // exits non-zero here before reading a licence, and reading that as a
        // violation blocks a push over a layout somebody chose.
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = []\n",
        );
        assert!(matches!(package_graph(tmp.path()), PackageGraph::Empty));
    }

    #[test]
    fn a_manifest_that_cannot_be_read_is_not_an_empty_one() {
        // The state that makes this an enum. A workspace naming a member that
        // does not exist used to block with the wrong reason, and skipping on it
        // instead would push with nothing checked and log that there was
        // nothing to check. Neither is honest; the graph is simply unknown.
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"nonexistent\"]\n",
        );
        assert!(matches!(
            package_graph(tmp.path()),
            PackageGraph::Unreadable
        ));
    }

    #[test]
    #[ignore = "needs cargo-deny"]
    fn an_unreadable_graph_blocks_rather_than_skipping() {
        require_cargo_deny();
        // And the caller has to act on the distinction, not merely receive it.
        // End-to-end through `check()`, which returns `Ok` with no work done
        // the moment cargo-deny is absent, so this needs the tool installed to
        // exercise the loop at all; `decide_root_action` covers the same
        // distinction without it, in `decide_root_action_blocks_only_a_non_spike_unreadable_graph`.
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("deny.toml"), "[licenses]\n");
        write(
            &tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"nonexistent\"]\n",
        );
        let err = check(tmp.path(), true).expect_err("an unreadable graph has to block");
        assert!(
            err.contains("could not read"),
            "blocked, but named the wrong reason: {err}"
        );
    }

    #[test]
    fn a_spike_tree_is_named_by_a_whole_path_component() {
        let spike = |p: &str| is_a_spike_tree(Path::new(p));

        // Positive: a `research` component, plain or nested under it,
        // including a sketch, which lives under `research/sketches/`.
        assert!(spike("mock/research/202608151700_probes/03_csv"));
        assert!(spike("mock/research/sketches/a_topic"));
        assert!(spike("research"));

        // Negative, and the substring case that a naive `contains` would get
        // wrong: a shipped crate whose name merely starts with `research` is
        // not a spike tree, and blocking is the right answer there.
        assert!(!spike("mock/crates/researcher"));
        assert!(!spike("mock/crates/bench-core"));
        assert!(!spike(""));

        // `sketches` on its own, with no `research` component above it, is
        // not a spike tree. The skip is unconditional now (finding 4), so a
        // name this wide would silently exempt a legitimately shipped
        // `sketches` crate outside `research/` from the whole gate.
        assert!(!spike("tools/sketches/x"));
        assert!(!spike("mock/crates/sketches-of-spain/src"));

        // The root itself, which is what `under` answers when a path does not
        // sit below the repo. Not a spike tree, so such a root blocks.
        assert!(!spike("."));
    }

    #[test]
    fn a_root_outside_the_repo_is_not_read_as_a_spike_tree() {
        // `under` is what keeps a component from above the repository out of
        // the decision. Without it an absolute path whose parent directory
        // happens to be called `research` would skip the gate.
        let outside = Path::new("/home/research/thing");
        assert!(is_a_spike_tree(outside), "the component really is there");
        assert!(!is_a_spike_tree(&under(Path::new("/srv/repo"), outside)));
    }

    // `decide_root_action` needs neither `cargo` nor `cargo-deny`: it is a
    // pure function over an already-computed `PackageGraph`, so every branch
    // runs unconditionally, unlike the `check()`-level tests below that need
    // the tool installed to reach the loop at all.

    #[test]
    fn decide_root_action_skips_a_spike_tree_whatever_its_graph_state() {
        // The behaviour finding 4 moved to the top of the loop: `spike`
        // decides the outcome outright, so every `PackageGraph` variant
        // driven through `spike = true` comes back the same way.
        for graph in [PackageGraph::Present, PackageGraph::Empty, PackageGraph::Unreadable] {
            assert_eq!(
                decide_root_action(true, graph),
                RootAction::Skip(SkipReason::SpikeTree),
                "a spike tree must skip regardless of its graph state"
            );
        }
    }

    #[test]
    fn decide_root_action_skips_a_non_spike_empty_graph() {
        assert_eq!(
            decide_root_action(false, PackageGraph::Empty),
            RootAction::Skip(SkipReason::EmptyGraph)
        );
    }

    #[test]
    fn decide_root_action_blocks_only_a_non_spike_unreadable_graph() {
        assert_eq!(
            decide_root_action(false, PackageGraph::Unreadable),
            RootAction::Block
        );
    }

    #[test]
    fn decide_root_action_runs_deny_on_a_non_spike_present_graph() {
        assert_eq!(
            decide_root_action(false, PackageGraph::Present),
            RootAction::RunDeny
        );
    }

    #[test]
    #[ignore = "needs cargo-deny"]
    fn an_unreadable_spike_tree_is_skipped_while_an_unreadable_crate_blocks() {
        require_cargo_deny();
        // The case a single-root fixture cannot express, and the reason this
        // one is built with four. Measured on mockspace itself: 23 roots, 7
        // unreadable, every one of them a committed probe under
        // `mock/research/`. Blocking on those gates the repository on the state
        // of its own audit trail, so the reaction has to depend on where the
        // root sits rather than only on what cargo said about it.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("deny.toml"), "[licenses]\n");
        // a readable root, so the loop reaches the others
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = []\n",
        );
        // unreadable, and part of the audit trail
        write(
            &root.join("mock/research/p1/Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"nonexistent\"]\n",
        );
        // unreadable, and shipped
        write(
            &root.join("mock/crates/shipped/Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"nonexistent\"]\n",
        );

        // The shipped one still blocks: the point is not that unreadable became
        // harmless.
        let err = check(root, true).expect_err("an unreadable shipped root has to block");
        assert!(
            err.contains("mock/crates/shipped"),
            "blocked on the wrong root: {err}"
        );

        // With the shipped one removed, the spike tree alone does not block,
        // and says which root it skipped.
        fs::remove_dir_all(root.join("mock/crates")).unwrap();
        let actions = check(root, true).expect("a spike tree alone must not block");
        assert!(
            actions
                .iter()
                .any(|a| a.contains("mock/research/p1") && a.contains("spike tree")),
            "skipped silently rather than saying which root and why: {actions:?}"
        );

        // Every root accounted for, one line each. The assertion above passes on
        // an implementation that reaches the spike tree and drops a root
        // somewhere else in the loop, and dropping one is the failure that looks
        // exactly like success: the gate reports Ok and says nothing about the
        // root it never checked.
        assert_eq!(
            actions.len(),
            workspace_roots(root).len(),
            "a root produced no action line, so it was dropped rather than \
             checked or skipped: {actions:?}"
        );
    }

    /// A stand-in for `cargo` that records its arguments one to a line in `argv`
    /// beside it, writes `stderr` to its standard error, `stdout` to its standard
    /// output, and exits with `code`. Whatever it prints, the gate has to account
    /// for: nothing it prints may reach the caller's own streams on a pass, and a
    /// failure has to carry what explains it. Like cargo-deny, it puts the
    /// diagnostics on stderr and the closing summary line on stdout.
    fn stub_cargo(dir: &Path, stdout: &str, stderr: &str, code: i32) -> PathBuf {
        let path = dir.join("stub-cargo");
        let script = format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > '{argv}'\ncat <<'OUT'\n{stdout}OUT\ncat >&2 <<'ERR'\n{stderr}ERR\nexit {code}\n",
            argv = dir.join("argv").display(),
        );
        crate::test_exec::write_executable(&path, script.as_bytes());
        path
    }

    /// Runs the stub in `dir` the way the gate runs `cargo`.
    fn run_stub(dir: &Path, cargo: &Path) -> Result<(), String> {
        run_deny_with(cargo.as_os_str(), dir, &dir.join("deny.toml"))
    }

    /// What cargo-deny prints for a graph with one rejected licence among many
    /// warnings: the diagnostics rendered one after another, on stderr.
    fn deny_diagnostics_with_one_error() -> String {
        let mut out = String::new();
        for n in 0 .. 300 {
            out.push_str(&format!(
                "warning[duplicate]: found 2 duplicate entries for crate 'dup{n}'\n   \u{250c}\u{2500} Cargo.lock:{n}:1\n   \u{2502}\n   \u{2502} dup{n} 1.0.0\n\n"
            ));
        }
        out.push_str(
            "error[rejected]: failed to satisfy license requirements\n   \u{250c}\u{2500} one/Cargo.toml:5:11\n   \u{2502}\n 5 \u{2502} license = \"MIT\"\n   \u{2502}           ^^^ rejected: license is not explicitly allowed\n\n",
        );
        out.push_str("warning[license-not-encountered]: license was not encountered\n\n");
        out
    }

    /// The summary line, which cargo-deny 0.20 writes to stdout.
    const DENY_SUMMARY: &str = "advisories ok, bans ok, licenses FAILED, sources ok\n";

    #[test]
    #[cfg(target_os = "linux")]
    fn a_clean_check_leaves_the_callers_streams_alone_whatever_it_printed() {
        // The stub notes in a file when its stderr or stdout is the very stream its
        // parent has, which is what inheriting them is. Returning Ok(()) says
        // nothing of that, since Ok(()) carries nothing: a pass that inherited
        // stderr poured tens of thousands of lines of warnings into a push. The
        // stub reads its own streams through `/proc/$$` and not `/proc/self`,
        // because inside a command substitution `/proc/self` is the `readlink`
        // process, whose stdout is the substitution's pipe and never the parent's.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("stub-cargo");
        let script = format!(
            "#!/usr/bin/env bash\nfor fd in 1 2; do\n  if [ \"$(readlink /proc/$$/fd/$fd)\" = \"$(readlink /proc/$PPID/fd/$fd)\" ]; then echo shared >> '{shared}'; fi\ndone\necho noise\necho noise >&2\nexit 0\n",
            shared = tmp.path().join("shared").display(),
        );
        crate::test_exec::write_executable(&path, script.as_bytes());
        assert_eq!(run_stub(tmp.path(), &path), Ok(()));
        assert!(
            !tmp.path().join("shared").exists(),
            "the check shared the caller's own stream, so what it printed on a pass reached it"
        );
    }

    #[test]
    fn the_check_is_run_with_plain_colour_and_the_one_config() {
        // `--color never` is what keeps the headers the filter reads plain text
        // when CARGO_TERM_COLOR forces colour, and `check` with `--config` is what
        // makes it the repo's one config over every root. A stub that ignores its
        // arguments passes whatever they are, so they are read back.
        let tmp = tempfile::tempdir().unwrap();
        let cargo = stub_cargo(tmp.path(), "", "", 0);
        assert_eq!(run_stub(tmp.path(), &cargo), Ok(()));
        let argv = std::fs::read_to_string(tmp.path().join("argv")).unwrap();
        let config = tmp.path().join("deny.toml");
        assert_eq!(argv.lines().collect::<Vec<_>>(), [
            "deny",
            "--color",
            "never",
            "--config",
            config.to_str().unwrap(),
            "check"
        ]);
    }

    #[test]
    fn a_failed_check_carries_its_errors_its_summary_and_its_status_and_no_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo = stub_cargo(
            tmp.path(),
            DENY_SUMMARY,
            &deny_diagnostics_with_one_error(),
            4,
        );
        let err = run_stub(tmp.path(), &cargo).expect_err("a non-zero exit is a failed check");
        assert!(
            err.contains("error[rejected]") && err.contains("license = \"MIT\""),
            "the error that explains the failure is missing:\n{err}"
        );
        assert!(
            err.contains("licenses FAILED"),
            "the summary line, which is on stdout, is missing:\n{err}"
        );
        assert!(
            err.contains("exited with status 4"),
            "the exit status is missing:\n{err}"
        );
        assert!(
            !err.contains("warning[duplicate]") && !err.contains("dup299"),
            "warnings reached the failure:\n{err}"
        );
        assert!(
            !err.contains("license-not-encountered"),
            "a warning after the error reached the failure:\n{err}"
        );
    }

    #[test]
    fn an_error_block_ends_at_the_next_header_of_any_severity() {
        // Two error blocks with a note, a help and a warning between and after
        // them: what is kept is the two blocks, and none of the other three, which
        // is the difference between `note` and `help` being headers and being
        // lines of the error above them.
        let tmp = tempfile::tempdir().unwrap();
        let stderr = "error[a]: first\n  first body\nnote[x]: a note\n  note body\nhelp: a help\n  help body\nerror[b]: second\n  second body\nwarning: a warning\n  warning body\n";
        let cargo = stub_cargo(tmp.path(), "", stderr, 4);
        let err = run_stub(tmp.path(), &cargo).expect_err("a non-zero exit is a failed check");
        for kept in ["error[a]: first", "first body", "error[b]: second", "second body"] {
            assert!(err.contains(kept), "{kept:?} was dropped:\n{err}");
        }
        for dropped in ["a note", "note body", "a help", "help body", "a warning", "warning body"] {
            assert!(!err.contains(dropped), "{dropped:?} was kept:\n{err}");
        }
    }

    #[test]
    fn a_failure_with_no_error_diagnostic_carries_exactly_the_last_forty_lines() {
        // cargo-deny dying on something that is not a diagnostic (a fetch, a
        // panic, a manifest it cannot parse) has no `error[` block to keep, and
        // dropping it all would turn the failure into one that says nothing.
        let tmp = tempfile::tempdir().unwrap();
        let noise: String = (1 ..= 100).map(|n| format!("noise-{n:03}\n")).collect();
        let cargo = stub_cargo(tmp.path(), "", &noise, 101);
        let err = run_stub(tmp.path(), &cargo).expect_err("a non-zero exit is a failed check");
        assert_eq!(
            err.lines().filter(|l| l.starts_with("noise-")).count(),
            FAILURE_TAIL_LINES,
            "{err}"
        );
        assert_eq!(FAILURE_TAIL_LINES, 40);
        assert!(
            err.contains("noise-100") && err.contains("noise-061"),
            "{err}"
        );
        assert!(
            !err.contains("noise-060"),
            "more than the tail was kept:\n{err}"
        );
        assert!(err.contains("exited with status 101"), "{err}");
    }

    #[test]
    fn a_failure_that_printed_nothing_still_names_its_status() {
        // The blank reason: cargo-deny killed before it said anything left the
        // person blocked by a push with an empty message.
        let tmp = tempfile::tempdir().unwrap();
        let cargo = stub_cargo(tmp.path(), "", "", 1);
        let err = run_stub(tmp.path(), &cargo).expect_err("a non-zero exit is a failed check");
        assert_eq!(err, "cargo deny exited with status 1\n");
    }

    #[test]
    fn a_cargo_that_cannot_run_is_a_failure_that_names_it() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_deny_with(
            tmp.path().join("no-such-cargo").as_os_str(),
            tmp.path(),
            &tmp.path().join("deny.toml"),
        )
        .expect_err("a program that is not there did not pass");
        assert!(err.contains("could not run"), "said:\n{err}");
    }

    #[test]
    fn the_dependency_gate_runs_on_a_push_and_stands_down_only_on_the_hooks_claim() {
        // Off: not a push, or switched off in the config. Run: a push with no claim
        // made, which is every hand run and every hook generated before the claim
        // existed. Stand down: a push the hook says changes no dependency input.
        assert_eq!(gate(true, true, Claim::None), Gate::Run);
        assert_eq!(gate(true, true, Claim::DepsUnchanged), Gate::StandDown);
        assert_eq!(gate(false, true, Claim::None), Gate::Off);
        assert_eq!(gate(false, true, Claim::DepsUnchanged), Gate::Off);
        assert_eq!(gate(true, false, Claim::None), Gate::Off);
        assert_eq!(gate(true, false, Claim::DepsUnchanged), Gate::Off);
        assert_eq!(gate(false, false, Claim::None), Gate::Off);
        assert_eq!(gate(false, false, Claim::DepsUnchanged), Gate::Off);
    }

    #[test]
    fn a_workspace_with_a_member_has_one() {
        // The control. Without it the test above passes on an implementation
        // that answers false for everything, which would skip the gate on every
        // repository rather than on the one that cannot run it.
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"one\"]\n",
        );
        write(
            &tmp.path().join("one/Cargo.toml"),
            "[package]\nname = \"one\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        );
        write(&tmp.path().join("one/src/lib.rs"), "");
        assert!(matches!(package_graph(tmp.path()), PackageGraph::Present));
    }

    /// Writes the shared fixture for the two tests below: a `deny.toml` that
    /// allows no licence at all, and one real crate carrying `license =
    /// "MIT"` (so the violation is "MIT is not on the allow list", not "no
    /// licence field", which is what an earlier version of this fixture
    /// actually failed on). `member_root` is where the crate's own workspace
    /// sits, relative to `root`: `mock/research/p1` for the spike case, `.`
    /// for the non-spike control.
    fn write_license_denial_fixture(root: &Path, member_root: &str) {
        write(&root.join("deny.toml"), "[licenses]\nallow = []\n");
        let member = root.join(member_root);
        write(
            &member.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"one\"]\n",
        );
        write(
            &member.join("one/Cargo.toml"),
            "[package]\nname = \"one\"\nversion = \"0.0.0\"\nedition = \"2021\"\nlicense = \"MIT\"\n",
        );
        write(&member.join("one/src/lib.rs"), "");
    }

    /// Runs `cargo deny check licenses` directly against the fixture, rather
    /// than through `check()` / `run_deny` (which run every category,
    /// including advisories, and can spend real time on a first fetch of the
    /// advisory database). Licence checking reads only the crate's own
    /// manifest and `deny.toml`, so this needs no network, and the returned
    /// output lets a test assert on the specific reason rather than trusting
    /// a bare exit code: a network failure elsewhere would not produce this
    /// text.
    fn license_check_output(member_root: &Path, config: &Path) -> std::process::Output {
        Command::new("cargo")
            .arg("deny")
            .arg("--config")
            .arg(config)
            .arg("check")
            .arg("licenses")
            .current_dir(member_root)
            .output()
            .expect("cargo deny check licenses did not run at all")
    }

    #[test]
    fn a_spike_tree_root_has_a_readable_package_graph() {
        // Needs only `cargo`, not `cargo-deny`: this is the state that made
        // the reported defect possible in the first place, and it holds
        // whether or not cargo-deny is installed to check it end to end.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_license_denial_fixture(root, "mock/research/p1");
        assert!(matches!(
            package_graph(&root.join("mock/research/p1")),
            PackageGraph::Present
        ));
    }

    #[test]
    #[ignore = "needs cargo-deny"]
    fn a_spike_tree_with_a_readable_graph_that_fails_deny_is_still_skipped() {
        require_cargo_deny();
        // The reported defect: `cargo metadata --no-deps` can come back
        // Present (readable) on a spike whose git dependency no longer
        // resolves, because `--no-deps` never walks the dependency it would
        // have failed on. `cargo deny check` does the full resolution and
        // fails there instead, on the same tree the `Unreadable` arm used to
        // be the only one that exempted. `decide_root_action_skips_a_spike_
        // tree_whatever_its_graph_state` covers the same shape purely; this
        // one checks it end to end through `check()`.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_license_denial_fixture(root, "mock/research/p1");
        let config = root.join("deny.toml");

        // The specific reason `cargo deny check` would fail here, established
        // independently of `check()` and without touching the network: `one`
        // carries `license = "MIT"`, `deny.toml` allows nothing, so the
        // licence check names both. A run that failed instead because the
        // network was unavailable would not print either.
        let licenses = license_check_output(&root.join("mock/research/p1"), &config);
        assert!(
            !licenses.status.success(),
            "the fixture's own crate should fail its licence check"
        );
        let stderr = String::from_utf8_lossy(&licenses.stderr);
        assert!(
            stderr.contains("rejected") && stderr.contains("MIT") && stderr.contains("one"),
            "did not fail for the reason the fixture is built to cause: {stderr}"
        );

        let actions = check(root, true)
            .expect("a spike tree with a readable but deny-failing graph must not block");
        assert!(
            actions
                .iter()
                .any(|a| a.contains("mock/research/p1") && a.contains("spike tree")),
            "did not skip the spike tree once its graph read as present: {actions:?}"
        );
    }

    #[test]
    fn a_non_spike_root_has_a_readable_package_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_license_denial_fixture(root, ".");
        assert!(matches!(package_graph(root), PackageGraph::Present));
    }

    #[test]
    #[ignore = "needs cargo-deny"]
    fn a_non_spike_root_with_a_readable_graph_that_fails_deny_still_blocks() {
        require_cargo_deny();
        // The control for the spike-tree test above. Without it, a version
        // that skipped every readable-but-failing root (not only spike trees)
        // would pass the positive case too.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_license_denial_fixture(root, ".");
        let config = root.join("deny.toml");

        let licenses = license_check_output(root, &config);
        assert!(
            !licenses.status.success(),
            "the fixture's own crate should fail its licence check"
        );
        let stderr = String::from_utf8_lossy(&licenses.stderr);
        assert!(
            stderr.contains("rejected") && stderr.contains("MIT") && stderr.contains("one"),
            "did not fail for the reason the fixture is built to cause: {stderr}"
        );

        let err = check(root, true)
            .expect_err("a non-spike root failing cargo deny check has to block the push");
        assert!(
            err.contains("cargo deny check failed"),
            "blocked, but named the wrong reason: {err}"
        );
    }

    #[test]
    #[ignore = "needs cargo-deny"]
    fn a_real_failure_under_forced_colour_still_carries_its_error_and_its_summary() {
        require_cargo_deny();
        // The failure the filter had under CARGO_TERM_COLOR=always: every header
        // opens with an escape, none matched, and the tail of the stream, all of it
        // warnings, was shown in place of the error. The fixture therefore has to
        // print warnings after the error, so that a filter that matches nothing
        // leaves a tail of warnings and no error: nineteen allowed licences that
        // the one crate never uses, each an unused-allowance warning. The program
        // run is a wrapper that forces colour in the environment and hands every
        // argument on, which is the one thing a stand-in `cargo` printing plain
        // text cannot show.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_license_denial_fixture(root, ".");
        let unused = [
            "Apache-2.0",
            "BSD-2-Clause",
            "BSD-3-Clause",
            "ISC",
            "Unicode-3.0",
            "Zlib",
            "0BSD",
            "CC0-1.0",
            "MPL-2.0",
            "Unlicense",
            "BSL-1.0",
            "OpenSSL",
            "CDLA-Permissive-2.0",
            "LGPL-3.0-only",
            "GPL-3.0-only",
            "AGPL-3.0-only",
            "EPL-2.0",
            "Python-2.0",
            "BlueOak-1.0.0",
        ];
        let allow = unused
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(", ");
        write(
            &root.join("deny.toml"),
            &format!("[licenses]\nallow = [{allow}]\n"),
        );
        let wrapper = root.join("coloured-cargo");
        crate::test_exec::write_executable(
            &wrapper,
            b"#!/usr/bin/env bash\nexport CARGO_TERM_COLOR=always\nexec cargo \"$@\"\n",
        );
        let err = run_deny_with(wrapper.as_os_str(), root, &root.join("deny.toml"))
            .expect_err("the fixture's crate fails its licence check");
        assert!(
            err.contains("error[rejected]"),
            "the error is missing:\n{err}"
        );
        assert!(
            err.contains("licenses FAILED"),
            "the summary line, which is on stdout, is missing:\n{err}"
        );
        assert!(
            err.contains("exited with status 4"),
            "the status is missing:\n{err}"
        );
        assert!(
            !err.contains('\u{1b}'),
            "colour reached the failure:\n{err}"
        );
        assert!(
            !err.contains("warning["),
            "a warning reached the failure of a check that had an error:\n{err}"
        );
    }
}
