//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a maker wrote, observed by the engine rather than reported by the tool.
//!
//! # Why observed
//!
//! A maker declares the paths it writes, and the declaration means something
//! only if it is compared against what actually changed. A list the tool hands
//! back would be the thing under audit and the evidence for it at once, so the
//! engine looks at the tree itself, before and after the run.
//!
//! # How, and what it cannot see
//!
//! `git status --porcelain -z --untracked-files=all` names every path that
//! differs from `HEAD` or is untracked and not ignored. Each such path is
//! fingerprinted by its content before the run and again after, and a path
//! whose fingerprint differs, or that is in one set and not the other, was
//! written. That catches a clean file made dirty, a dirty file changed again, a
//! new file, a deletion, and a dirty file put back to what `HEAD` holds.
//!
//! It does not see three things, and they are stated rather than implied:
//!
//! - a path git ignores, since status never names it, so a maker writing into
//!   `target/` or another ignored tree is not held to its declaration there;
//! - a write that leaves the bytes exactly as they were, which is no change;
//! - a tree with no git at all, where [`snapshot`] returns `None` and the
//!   caller says the writes went unaudited.
//!
//! Anything else writing to the same worktree during the run is attributed to
//! the maker, since the observation cannot tell writers apart.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

/// The dirty and untracked paths of a worktree, each with a fingerprint of its
/// content, or `None` where the path no longer exists.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot(BTreeMap<String, Option<u64>>);

/// Take a snapshot of `repo_root`, or `None` where git cannot answer for it.
///
/// Paths are relative to `repo_root` and `/`-separated, which is the form a
/// maker's declared patterns are matched against. Git reports them relative
/// to its own top level, so where `repo_root` sits below that, they are
/// rebased; a path above `repo_root` keeps its top-level spelling with a `../`
/// prefix per level, which no declaration can match, so it is reported.
pub(crate) fn snapshot(repo_root: &Path) -> Option<Snapshot> {
    let top = git(repo_root, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim_end_matches('\n')).to_path_buf();
    let raw = git(repo_root, &[
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    ])?;
    let root = repo_root.canonicalize().ok()?;
    let top = top.canonicalize().ok()?;

    let mut map = BTreeMap::new();
    for rel in porcelain_paths(&raw) {
        let abs = top.join(&rel);
        let key = match abs.strip_prefix(&root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => {
                let depth = root
                    .strip_prefix(&top)
                    .map(|p| p.components().count())
                    .unwrap_or(0);
                format!("{}{rel}", "../".repeat(depth))
            },
        };
        map.insert(key, fingerprint(&abs));
    }
    Some(Snapshot(map))
}

/// Every path that differs between two snapshots, sorted.
pub(crate) fn written(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out: Vec<String> = after
        .0
        .iter()
        .filter(|(p, h)| before.0.get(*p) != Some(*h))
        .map(|(p, _)| p.clone())
        .collect();
    // Dirty before and clean after: the run put it back to `HEAD`, which is a
    // write all the same.
    out.extend(
        before
            .0
            .keys()
            .filter(|p| !after.0.contains_key(*p))
            .cloned(),
    );
    out.sort();
    out.dedup();
    out
}

/// The paths named in `git status --porcelain=v1 -z` output.
///
/// Each record is `XY path`, NUL-terminated; a rename or copy carries its
/// source as a second NUL-terminated field, which is a path the run may have
/// touched too, so both are kept.
fn porcelain_paths(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fields = raw.split('\0').filter(|f| !f.is_empty());
    while let Some(rec) = fields.next() {
        let Some(path) = rec.get(3 ..) else {
            continue;
        };
        out.push(path.to_string());
        if matches!(rec.as_bytes().first(), Some(b'R' | b'C')) {
            if let Some(src) = fields.next() {
                out.push(src.to_string());
            }
        }
    }
    out
}

/// A content fingerprint, or `None` where the path is gone or unreadable.
///
/// Not cryptographic, and it does not need to be: it is compared only against
/// itself across one run, to ask whether these bytes changed.
fn fingerprint(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Some(h.finish())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .arg("-C")
                .arg(tmp.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        fs::write(tmp.path().join("tracked.md"), "one\n").unwrap();
        fs::write(tmp.path().join("dirty.md"), "one\n").unwrap();
        fs::write(tmp.path().join(".gitignore"), "ignored/\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        fs::write(tmp.path().join("dirty.md"), "two\n").unwrap();
        tmp
    }

    #[test]
    fn every_kind_of_write_is_seen_and_nothing_else_is() {
        let tmp = repo();
        let root = tmp.path();
        fs::write(root.join("untouched-new.md"), "here before\n").unwrap();
        let before = snapshot(root).expect("a git tree answers");

        fs::write(root.join("tracked.md"), "changed\n").unwrap(); // clean to dirty
        fs::write(root.join("dirty.md"), "three\n").unwrap(); // dirty, changed again
        fs::create_dir_all(root.join("gen")).unwrap();
        fs::write(root.join("gen/new.md"), "made\n").unwrap(); // new
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/x"), "unseen\n").unwrap(); // ignored, unseen

        let after = snapshot(root).unwrap();
        assert_eq!(
            written(&before, &after),
            vec!["dirty.md", "gen/new.md", "tracked.md"],
            "an untracked file present and unchanged before the run is not a write"
        );
    }

    #[test]
    fn a_deletion_and_a_revert_to_head_are_writes() {
        let tmp = repo();
        let root = tmp.path();
        let before = snapshot(root).unwrap();
        fs::remove_file(root.join("tracked.md")).unwrap();
        fs::write(root.join("dirty.md"), "one\n").unwrap(); // back to HEAD
        let after = snapshot(root).unwrap();
        assert_eq!(written(&before, &after), vec!["dirty.md", "tracked.md"]);
    }

    #[test]
    fn a_run_that_writes_nothing_writes_nothing() {
        let tmp = repo();
        let before = snapshot(tmp.path()).unwrap();
        let after = snapshot(tmp.path()).unwrap();
        assert!(written(&before, &after).is_empty());
    }

    #[test]
    fn a_tree_without_git_has_no_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(snapshot(tmp.path()), None);
    }

    #[test]
    fn a_rename_names_both_paths() {
        assert_eq!(porcelain_paths("R  new.md\0old.md\0?? x.md\0"), vec![
            "new.md", "old.md", "x.md"
        ]);
    }
}
