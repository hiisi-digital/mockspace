//! Pre-commit auto-formatting and clippy-fix, scoped to the staged files.
//!
//! When a commit is being made, the fixers touch **only the files being
//! committed**, the lint-staged / prettier + eslint pre-commit model. Nothing
//! outside the commit changes, so no unrelated file is rewritten and then
//! blocked by the design-round phase gate.
//!
//! - `auto_fmt` runs `rustfmt` directly on the staged `.rs` files (those that
//!   still exist; a staged deletion is skipped). rustfmt resolves `rustfmt.toml`
//!   by walking the directory tree upward, so a repo-root config governs `mock/`
//!   sources too.
//! - `auto_clippy_fix` is crate-level (clippy cannot target single files), so it
//!   runs `cargo clippy --fix` in each changed package, then **reverts** any file
//!   clippy touched that is not part of the commit and was not already dirty.
//!   Only the staged files keep their fixes; a pristine sibling clippy happened
//!   to rewrite is restored. A sibling that was already dirty (the user's own
//!   in-flight edit) is left alone and surfaced, never clobbered.
//!
//! The fixers use whatever config the repo already has (`rustfmt.toml`, the
//! `#![warn(clippy::…)]` crate attributes); this module never imposes a style.
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
/// caller to log. A no-op (and empty result) when both flags are off or nothing
/// is staged.
pub fn run(repo_root: &Path, auto_fmt: bool, auto_clippy_fix: bool) -> Vec<String> {
    let mut actions = Vec::new();
    if !auto_fmt && !auto_clippy_fix {
        return actions;
    }

    let staged = git_lines(repo_root, &["diff", "--cached", "--name-only"]);
    if staged.is_empty() {
        return actions;
    }
    let staged_set: BTreeSet<PathBuf> = staged.iter().cloned().collect();
    // Files already dirty before the fixers ran. Two uses: they must not be
    // re-staged (a `git add -p` partial would sweep in withheld edits), and a
    // clippy rewrite of one is the user's concern, never reverted.
    let pre_dirty: BTreeSet<PathBuf> = git_lines(repo_root, &["diff", "--name-only"])
        .into_iter()
        .collect();

    // auto_fmt: rustfmt ONLY the staged .rs files that still exist on disk.
    if auto_fmt {
        let files: Vec<PathBuf> = staged
            .iter()
            .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
            .filter(|p| repo_root.join(p).is_file())
            .cloned()
            .collect();
        if !files.is_empty() {
            if run_rustfmt(repo_root, &files) {
                actions.push(format!("auto_fmt: rustfmt {} staged file(s)", files.len()));
            } else {
                actions.push("auto_fmt: skipped, rustfmt failed".to_string());
            }
        }
    }

    // auto_clippy_fix: crate-level, so run per changed package then revert any
    // file clippy touched that is not staged and was not already dirty.
    if auto_clippy_fix {
        let pkgs = changed_package_dirs(&staged, repo_root, |p| p.is_file());
        let mut ran = false;
        for pkg in &pkgs {
            ran |= run_cargo(pkg, &["clippy", "--fix", "--allow-dirty", "--allow-staged"]);
        }
        if ran {
            let post_dirty: BTreeSet<PathBuf> = git_lines(repo_root, &["diff", "--name-only"])
                .into_iter()
                .collect();
            let spurious = spurious_files(&post_dirty, &staged_set, &pre_dirty);
            let reverted = revert(repo_root, &spurious);
            actions.push(format!(
                "auto_clippy_fix: cargo clippy --fix ({} package(s)); reverted {reverted} unrelated file(s)",
                pkgs.len()
            ));
        }
    }

    // Re-stage the safe set: staged files (with no pre-existing unstaged edit)
    // the fixers may have rewritten, so the fixed content is what the commit
    // records. Partially-staged files are left as-is to preserve `git add -p`.
    let (safe_to_restage, partially_staged) = partition_restage(&staged, &pre_dirty);
    let restaged = restage(repo_root, &safe_to_restage);
    if restaged > 0 {
        actions.push(format!("re-staged {restaged} file(s) after fixers"));
    }
    if !partially_staged.is_empty() {
        actions.push(format!(
            "auto-fix left {} partially-staged file(s) untouched (re-stage manually if reformatted)",
            partially_staged.len()
        ));
    }
    actions
}

/// Files a fixer dirtied that are neither part of the commit nor previously
/// dirty: the pristine siblings clippy rewrote, which must be reverted so only
/// the staged files change. Pure over the three sets for testability.
fn spurious_files(
    post_dirty: &BTreeSet<PathBuf>,
    staged: &BTreeSet<PathBuf>,
    pre_dirty: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    post_dirty
        .iter()
        .filter(|f| !staged.contains(*f) && !pre_dirty.contains(*f))
        .cloned()
        .collect()
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

/// Run `rustfmt` on the given repo-relative files. rustfmt discovers
/// `rustfmt.toml` (edition included) by walking up from each file, so the repo's
/// own style applies. `true` on exit status 0.
fn run_rustfmt(repo_root: &Path, files: &[PathBuf]) -> bool {
    Command::new("rustfmt")
        .args(files)
        .current_dir(repo_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `git checkout -- <files>` to restore working-tree files to their staged/HEAD
/// content. Returns the count restored.
fn revert(repo_root: &Path, files: &[PathBuf]) -> usize {
    let mut n = 0;
    for f in files {
        let ok = Command::new("git")
            .arg("checkout")
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

    #[test]
    fn spurious_is_dirtied_minus_staged_minus_predirty() {
        let set = |v: &[&str]| -> BTreeSet<PathBuf> { v.iter().map(PathBuf::from).collect() };
        // clippy dirtied a staged file, a pre-existing dirty sibling, and a
        // pristine sibling. Only the pristine sibling is spurious (to revert).
        let post = set(&["staged.rs", "already_dirty.rs", "pristine_sibling.rs"]);
        let staged = set(&["staged.rs"]);
        let pre = set(&["already_dirty.rs"]);
        assert_eq!(spurious_files(&post, &staged, &pre), vec![PathBuf::from(
            "pristine_sibling.rs"
        )]);
    }

    #[test]
    fn spurious_empty_when_only_staged_changed() {
        let set = |v: &[&str]| -> BTreeSet<PathBuf> { v.iter().map(PathBuf::from).collect() };
        let post = set(&["staged.rs"]);
        let staged = set(&["staged.rs"]);
        assert!(spurious_files(&post, &staged, &BTreeSet::new()).is_empty());
    }
}
