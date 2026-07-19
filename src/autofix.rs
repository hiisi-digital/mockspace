//! Pre-commit auto-formatting and clippy-fix.
//!
//! When a commit is being made, run the repo's own configured `cargo fmt` and
//! (optionally) `cargo clippy --fix` in each package the staged changes touch
//! (a file's nearest ancestor `Cargo.toml` directory), then re-stage the
//! results so the commit lands already-fixed. Scoping to the changed packages
//! keeps the cost proportional to the change: `cargo clippy --fix` only
//! compiles those packages, not the whole workspace.
//!
//! The fixers use whatever config the repo already has: `cargo fmt` resolves
//! `rustfmt.toml` by walking the directory tree upward (it ignores workspace
//! boundaries, so a repo-root `rustfmt.toml` governs `mock/` sources too), and
//! `cargo clippy --fix` respects the `#![warn(clippy::…)]` crate attributes the
//! entrypoints declare. This module never imposes a style; it applies the
//! project's own.
//!
//! Best-effort by design: a fixer that fails (rustfmt on unparseable source,
//! clippy on code that does not compile yet) is logged and skipped, never
//! blocking the commit. The lint gate that runs afterwards is the real bar.
//!
//! Opt out per-repo via `mockspace.toml`: `auto_fmt = false` /
//! `auto_clippy_fix = false` (both default true).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run the configured pre-commit fixers. `repo_root` is the repository top
/// level (the git work-tree root). Returns human-readable action lines for the
/// caller to log. A no-op (and empty result) when both flags are off, nothing
/// is staged, or no staged file belongs to a cargo package.
pub fn run(repo_root: &Path, auto_fmt: bool, auto_clippy_fix: bool) -> Vec<String> {
    let mut actions = Vec::new();
    if !auto_fmt && !auto_clippy_fix {
        return actions;
    }

    let staged = git_lines(repo_root, &["diff", "--cached", "--name-only"]);
    if staged.is_empty() {
        return actions;
    }
    // Files safe to re-stage after the fixers run: those staged with no
    // unstaged component at entry. Re-adding a partially-staged file (`git add
    // -p`) would sweep in edits the user deliberately withheld, so those are
    // left alone.
    let unstaged: BTreeSet<PathBuf> = git_lines(repo_root, &["diff", "--name-only"])
        .into_iter()
        .collect();
    let (safe_to_restage, partially_staged) = partition_restage(&staged, &unstaged);

    let pkgs = changed_package_dirs(&staged, repo_root, |p| p.is_file());
    if pkgs.is_empty() {
        return actions;
    }

    // Run the fixers in each changed package's own directory. cargo infers the
    // package from the cwd, so a bare `cargo fmt` / `cargo clippy --fix` scopes
    // to just that package (and, for clippy, only compiles that package and its
    // deps). The cost is proportional to what is being committed, not the whole
    // workspace, and members are covered because each maps to its own package.
    for pkg in &pkgs {
        let where_ = rel(repo_root, pkg);
        if auto_fmt {
            match run_cargo(pkg, &["fmt"]) {
                true => actions.push(format!("auto_fmt: cargo fmt ({where_})")),
                false => actions.push(format!("auto_fmt: skipped, cargo fmt failed ({where_})")),
            }
        }
        if auto_clippy_fix {
            // --allow-dirty/--allow-staged because the hook runs against staged
            // and possibly dirty files.
            let args = ["clippy", "--fix", "--allow-dirty", "--allow-staged"];
            match run_cargo(pkg, &args) {
                true => actions.push(format!("auto_clippy_fix: cargo clippy --fix ({where_})")),
                false => {
                    actions.push(format!(
                        "auto_clippy_fix: skipped, clippy --fix failed ({where_})"
                    ))
                },
            }
        }
    }

    // Re-stage the safe set: files the fixers may have rewritten in the working
    // tree, so the formatted content is what the commit records.
    let restaged = restage(repo_root, &safe_to_restage);
    if restaged > 0 {
        actions.push(format!("re-staged {restaged} file(s) after fixers"));
    }
    // Partially-staged files that changed under a fixer are surfaced so the user
    // knows the commit will NOT contain their reformat.
    if !partially_staged.is_empty() {
        actions.push(format!(
            "auto-fix left {} partially-staged file(s) untouched (re-stage manually if reformatted)",
            partially_staged.len()
        ));
    }
    actions
}

/// Split staged files into the set safe to re-stage (no unstaged component, so
/// re-adding records only the fixer's change) and the partially-staged set
/// (also has unstaged edits, so re-adding would sweep in withheld content). The
/// partial set is left untouched to preserve `git add -p` intent.
fn partition_restage(
    staged: &[PathBuf],
    unstaged: &BTreeSet<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut safe = Vec::new();
    let mut partial = Vec::new();
    for f in staged {
        if unstaged.contains(f) {
            partial.push(f.clone());
        } else {
            safe.push(f.clone());
        }
    }
    (safe, partial)
}

/// The unique package directories the given staged files belong to. Each file
/// resolves to its nearest ancestor directory holding a `Cargo.toml` (per
/// `has_manifest`), bounded at `repo_root`. Running a bare `cargo fmt` /
/// `cargo clippy --fix` there scopes the fixer to that one package, so the cost
/// is proportional to the change rather than the whole workspace. Files under no
/// manifest are dropped. Pure over `has_manifest` so the walk is unit-testable
/// without a filesystem.
fn changed_package_dirs(
    staged: &[PathBuf],
    repo_root: &Path,
    has_manifest: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for rel_file in staged {
        let abs = repo_root.join(rel_file);
        if let Some(dir) = package_dir_of(&abs, repo_root, &has_manifest) {
            dirs.insert(dir);
        }
    }
    dirs.into_iter().collect()
}

/// Walk up from a file to the nearest ancestor directory containing a
/// `Cargo.toml`, bounded at (and including) `repo_root`. `None` when no ancestor
/// up to the root has one.
fn package_dir_of(
    file_abs: &Path,
    repo_root: &Path,
    has_manifest: &impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let mut dir = file_abs.parent()?;
    loop {
        if has_manifest(&dir.join("Cargo.toml")) {
            return Some(dir.to_path_buf());
        }
        if dir == repo_root {
            return None;
        }
        dir = dir.parent()?;
    }
}

/// Run `cargo <args>` in `root`, discarding output. `true` on exit status 0.
fn run_cargo(root: &Path, args: &[&str]) -> bool {
    Command::new("cargo")
        .args(args)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `git add` the given repo-relative paths. Returns the count added.
fn restage(repo_root: &Path, files: &[PathBuf]) -> usize {
    let mut n = 0;
    for f in files {
        let ok = Command::new("git")
            .arg("add")
            .arg("--")
            .arg(f)
            .current_dir(repo_root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            n += 1;
        }
    }
    n
}

/// Run a git command in `repo_root` and return stdout as trimmed, non-empty,
/// path lines.
fn git_lines(repo_root: &Path, args: &[&str]) -> Vec<PathBuf> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect()
        },
        _ => Vec::new(),
    }
}

/// Display `path` relative to `repo_root`, falling back to the full path.
fn rel(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            if path == repo_root { ".".to_string() } else { path.display().to_string() }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `has_manifest` predicate true for the given `Cargo.toml` paths.
    fn manifests(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.contains(&p.to_str().unwrap())
    }

    #[test]
    fn package_dirs_dedupe_and_map_files_to_nearest_manifest() {
        let repo = Path::new("/repo");
        // Cargo.toml at the root (combined package+workspace) and at each member.
        let has = manifests(&[
            "/repo/Cargo.toml",
            "/repo/mock/crates/y/Cargo.toml",
            "/repo/mock/crates/x/Cargo.toml",
        ]);
        let staged = vec![
            PathBuf::from("src/lib.rs"),                // -> /repo
            PathBuf::from("src/entry/dispatch.rs"),     // -> /repo (dedup)
            PathBuf::from("mock/crates/y/src/main.rs"), // -> the y package
            PathBuf::from("mock/crates/x/src/lib.rs"),  // -> the x package
        ];
        let dirs = changed_package_dirs(&staged, repo, has);
        assert_eq!(dirs, vec![
            PathBuf::from("/repo"),
            PathBuf::from("/repo/mock/crates/x"),
            PathBuf::from("/repo/mock/crates/y"),
        ]);
    }

    #[test]
    fn member_maps_to_its_own_package_not_an_ancestor() {
        let repo = Path::new("/repo");
        // both the root and a member carry a Cargo.toml; a member file resolves
        // to the member package (nearest), so the fixer scopes to just it.
        let has = manifests(&["/repo/Cargo.toml", "/repo/mock/crates/z/Cargo.toml"]);
        let staged = vec![PathBuf::from("mock/crates/z/src/lib.rs")];
        assert_eq!(changed_package_dirs(&staged, repo, has), vec![
            PathBuf::from("/repo/mock/crates/z")
        ]);
    }

    #[test]
    fn file_under_no_manifest_is_dropped() {
        let repo = Path::new("/repo");
        let has = manifests(&[]); // no Cargo.toml anywhere
        let staged = vec![PathBuf::from("README.md")];
        assert!(changed_package_dirs(&staged, repo, has).is_empty());
    }

    #[test]
    fn nearest_manifest_is_found_up_the_tree() {
        let repo = Path::new("/repo");
        let has = manifests(&["/repo/Cargo.toml"]);
        let staged = vec![PathBuf::from("deep/nested/file.rs")];
        assert_eq!(changed_package_dirs(&staged, repo, has), vec![
            PathBuf::from("/repo")
        ]);
    }

    #[test]
    fn rel_renders_root_as_dot() {
        let repo = Path::new("/repo");
        assert_eq!(rel(repo, repo), ".");
        assert_eq!(rel(repo, &repo.join("mock")), "mock");
    }

    #[test]
    fn partition_restage_excludes_partially_staged_files() {
        // `a.rs` and `c.rs` are cleanly staged; `b.rs` is staged AND has
        // unstaged edits (a `git add -p` partial). `b.rs` must NOT be re-staged,
        // or the commit would sweep in edits the user deliberately withheld.
        let staged = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs"), PathBuf::from("c.rs")];
        let unstaged: BTreeSet<PathBuf> = [PathBuf::from("b.rs")].into_iter().collect();
        let (safe, partial) = partition_restage(&staged, &unstaged);
        assert_eq!(safe, vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")]);
        assert_eq!(partial, vec![PathBuf::from("b.rs")]);
    }

    #[test]
    fn partition_restage_all_clean_when_no_unstaged() {
        let staged = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let unstaged = BTreeSet::new();
        let (safe, partial) = partition_restage(&staged, &unstaged);
        assert_eq!(safe, staged);
        assert!(partial.is_empty());
    }
}
