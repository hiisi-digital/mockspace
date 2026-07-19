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
}
