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
//! Blocks the push when cargo-deny reports a violation. Skipped (never blocking)
//! when no `deny.toml` exists or cargo-deny is not installed. Opt out with
//! `deny_check = false` in `mockspace.toml`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run the deny gate. `Err(message)` when cargo-deny reports a violation in some
/// workspace root (the pre-push gate blocks on it). `Ok(actions)` otherwise,
/// including every skip path (config absent, tool absent, disabled), with the
/// action lines for the caller to log.
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
        // A workspace with no packages has no dependency graph, so cargo-deny
        // exits non-zero on `cargo metadata` before it looks at a single
        // licence. Reading that as a violation blocks a push over a repository
        // layout somebody chose on purpose, and says the wrong thing about why.
        match package_graph(&root) {
            PackageGraph::Empty => {
                actions.push(format!(
                    "deny_check: skipped, no package graph to check ({where_})"
                ));
                continue;
            },
            PackageGraph::Unreadable if is_a_spike_tree(&under(repo_root, &root)) => {
                // A probe or a sketch is a spike: it exists to check one thing,
                // it takes shortcuts everywhere, and nothing it depends on
                // reaches a consumer. Several here carry a `[workspace]` with
                // no target at all, so cargo has nothing to answer with. Say so
                // and carry on; blocking a push over one would gate the
                // repository on the state of its own audit trail.
                actions.push(format!(
                    "deny_check: skipped, no readable package graph in {where_} (a spike tree)"
                ));
                continue;
            },
            PackageGraph::Unreadable => {
                return Err(format!(
                    "deny_check: could not read the package graph in {where_}, so nothing was checked"
                ));
            },
            PackageGraph::Present => {},
        }
        if run_deny(&root, &config) {
            actions.push(format!("deny_check: cargo deny check passed ({where_})"));
        } else {
            return Err(format!(
                "cargo deny check failed in {where_} (license/advisory/ban/source violation)"
            ));
        }
    }
    Ok(actions)
}

/// Whether a root sits inside the audit trail rather than the shipped tree.
///
/// Keyed on a whole path component, so a crate named `research-tools` is not
/// caught by a crate named `research`. The two names are the ones the workspace
/// reserves for spikes: `mock/research/**` for a panel's probes and `sketches/`
/// for a feasibility check.
///
/// A first-party crate that genuinely lives at one of those names and whose
/// metadata is unreadable would be skipped rather than blocked. That is the
/// wrong answer in principle and a tolerable one in practice, because the
/// action line names the root it skipped and why, so the skip is readable
/// rather than silent.
///
/// Takes a path rather than the rendered message string, so what counts as a
/// component is the platform's answer and not whichever separator `display`
/// happened to use.
fn is_a_spike_tree(rel_path: &Path) -> bool {
    rel_path
        .components()
        .any(|c| c.as_os_str() == "research" || c.as_os_str() == "sketches")
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

/// Run `cargo deny --config <config> check` in `root`. `true` on a clean check.
fn run_deny(root: &Path, config: &Path) -> bool {
    Command::new("cargo")
        .arg("deny")
        .arg("--config")
        .arg(config)
        .arg("check")
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
    fn an_unreadable_graph_blocks_rather_than_skipping() {
        // And the caller has to act on the distinction, not merely receive it.
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("deny.toml"), "[licenses]\n");
        write(
            &tmp.path().join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"nonexistent\"]\n",
        );
        if !cargo_deny_installed() {
            // The skip path returns before the loop, so this arm would pass on
            // any implementation. Say so rather than asserting nothing.
            eprintln!("cargo-deny absent: the loop is unreachable, arm not exercised");
            return;
        }
        let err = check(tmp.path(), true).expect_err("an unreadable graph has to block");
        assert!(
            err.contains("could not read"),
            "blocked, but named the wrong reason: {err}"
        );
    }

    #[test]
    fn a_spike_tree_is_named_by_a_whole_path_component() {
        let spike = |p: &str| is_a_spike_tree(Path::new(p));

        // Positive, and both reserved names.
        assert!(spike("mock/research/202608151700_probes/03_csv"));
        assert!(spike("mock/research/sketches/a_topic"));
        assert!(spike("research"));

        // Negative, and the substring case that a naive `contains` would get
        // wrong: a shipped crate whose name merely starts with one of them is
        // not a spike tree, and blocking is the right answer there.
        assert!(!spike("mock/crates/researcher"));
        assert!(!spike("mock/crates/sketches-of-spain/src"));
        assert!(!spike("mock/crates/bench-core"));
        assert!(!spike(""));

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

    #[test]
    fn an_unreadable_spike_tree_is_skipped_while_an_unreadable_crate_blocks() {
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

        if !cargo_deny_installed() {
            eprintln!("cargo-deny absent: the loop is unreachable, arm not exercised");
            return;
        }

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
}
