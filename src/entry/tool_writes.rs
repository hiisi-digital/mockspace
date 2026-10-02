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
//! new file, a deletion, and a dirty file put back to what `HEAD` holds. A
//! second `git status --ignored`, restricted to the maker's declared patterns,
//! adds the ignored paths inside the declaration, so a maker whose output is
//! gitignored is seen writing it.
//!
//! It does not see these, and they are stated rather than implied:
//!
//! - an ignored path outside the declaration, since only the declared patterns
//!   are asked about with `--ignored`, so a maker writing into `target/` while
//!   declaring something else is not seen doing it;
//! - anything under `.git/`, which status never names, so a maker could
//!   rewrite hooks or config unseen; a declared pattern with a literal `.git`
//!   segment is refused for that reason, which closes the grant and not the
//!   blind spot;
//! - a write outside the worktree, to a home directory, a temp directory or
//!   another repository, since only this worktree is asked about;
//! - a write through a symlink, which lands wherever the link points and is
//!   seen, at most, as the link itself; a symlink on the literal path of a
//!   write rooted at an argument is refused before the run by
//!   `symlinked_writes`, which closes the case an argument chose and not one
//!   committed below its value or under a fixed pattern;
//! - a permission-only change to a file that was already dirty, since the
//!   fingerprint is of content, and a mode change to a clean file is seen only
//!   because status names the file afterwards;
//! - a write that leaves the bytes exactly as they were, which is no change.
//!
//! Where git cannot answer at all, there is no snapshot, and the engine
//! refuses to run a maker rather than run it unheld; an after-snapshot that
//! fails makes the run inconclusive, through [`after_run`].
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
///
/// `declared` is a maker's declared writes, with any argument they start with
/// already resolved from the command line. Ignored paths inside them are
/// observed too, through a second `git status --ignored` restricted to those
/// patterns as glob pathspecs, so a maker whose output is gitignored is seen
/// writing it, and an ignored tree outside the declaration is never walked.
pub(crate) fn snapshot(repo_root: &Path, declared: &[&str]) -> Option<Snapshot> {
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

    let mut rels = porcelain_paths(&raw);
    if !declared.is_empty() {
        let specs: Vec<String> = declared.iter().map(|d| format!(":(glob){d}")).collect();
        let mut args =
            vec!["status", "--porcelain=v1", "-z", "--ignored", "--untracked-files=all", "--"];
        args.extend(specs.iter().map(String::as_str));
        let ignored = git(repo_root, &args)?;
        rels.extend(ignored_paths(&ignored));
    }

    let mut map = BTreeMap::new();
    for rel in rels {
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

/// The ignored paths, `!!` records, in `git status --ignored -z` output.
fn ignored_paths(raw: &str) -> Vec<String> {
    raw.split('\0')
        .filter_map(|rec| rec.strip_prefix("!! "))
        .filter(|p| !p.ends_with('/'))
        .map(str::to_string)
        .collect()
}

/// What a run wrote, given the snapshot taken before it and the one after.
///
/// An after-snapshot of `None` is an error rather than an empty list: the run
/// happened and its tree could not be read back, so "wrote nothing" would be
/// a claim the engine never established. The caller reports it inconclusive.
pub(crate) fn after_run(before: &Snapshot, after: Option<Snapshot>) -> Result<Vec<String>, String> {
    match after {
        Some(a) => Ok(written(before, &a)),
        None => {
            Err(
                "git could not report the tree after the run, so what it wrote is unknown"
                    .to_string(),
            )
        },
    }
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
        let before = snapshot(root, &[]).expect("a git tree answers");

        fs::write(root.join("tracked.md"), "changed\n").unwrap(); // clean to dirty
        fs::write(root.join("dirty.md"), "three\n").unwrap(); // dirty, changed again
        fs::create_dir_all(root.join("gen")).unwrap();
        fs::write(root.join("gen/new.md"), "made\n").unwrap(); // new
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/x"), "unseen\n").unwrap(); // ignored, unseen

        let after = snapshot(root, &[]).unwrap();
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
        let before = snapshot(root, &[]).unwrap();
        fs::remove_file(root.join("tracked.md")).unwrap();
        fs::write(root.join("dirty.md"), "one\n").unwrap(); // back to HEAD
        let after = snapshot(root, &[]).unwrap();
        assert_eq!(written(&before, &after), vec!["dirty.md", "tracked.md"]);
    }

    #[test]
    fn a_declared_ignored_output_counts_as_written() {
        // The case that must fail: a maker whose only output is gitignored,
        // a build artifact or a report, must be seen writing it, or it reads
        // as having written nothing. Ignored paths are asked about only
        // inside the declaration, so an ignored tree is not walked whole.
        let tmp = repo();
        let root = tmp.path();
        let declared = ["ignored/**"];
        let before = snapshot(root, &declared).unwrap();
        fs::create_dir_all(root.join("ignored")).unwrap();
        fs::write(root.join("ignored/report.json"), "{}\n").unwrap();
        fs::write(root.join("ignored/other.bin"), "x\n").unwrap();
        let after = snapshot(root, &declared).unwrap();
        assert_eq!(written(&before, &after), vec![
            "ignored/other.bin",
            "ignored/report.json"
        ]);
        // and undeclared, an ignored write stays unseen, as before
        let before = snapshot(root, &[]).unwrap();
        fs::write(root.join("ignored/report.json"), "{\"changed\": 1}\n").unwrap();
        let after = snapshot(root, &[]).unwrap();
        assert!(written(&before, &after).is_empty());
    }

    #[test]
    fn a_run_that_writes_nothing_writes_nothing() {
        let tmp = repo();
        let before = snapshot(tmp.path(), &[]).unwrap();
        let after = snapshot(tmp.path(), &[]).unwrap();
        assert!(written(&before, &after).is_empty());
    }

    #[test]
    fn a_tree_without_git_has_no_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(snapshot(tmp.path(), &[]), None);
    }

    #[test]
    fn an_after_snapshot_that_failed_is_inconclusive() {
        // The case that must fail: a run whose tree could not be read back
        // afterwards must not read as one that wrote nothing.
        let tmp = repo();
        let before = snapshot(tmp.path(), &[]).unwrap();
        let got = after_run(&before, None);
        assert!(got.is_err(), "{got:?}");
        // and the control: a readable tree after the run is an answer
        let after = snapshot(tmp.path(), &[]);
        assert_eq!(after_run(&before, after), Ok(Vec::new()));
    }

    #[test]
    fn a_repo_root_below_the_top_level_sees_paths_above_it_as_leaving() {
        // Paths are rebased onto `repo_root`, and one above it keeps a `../`
        // per level, which no declaration can match because a declared
        // pattern with `..` is refused.
        let tmp = repo();
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        let before = snapshot(&root.join("sub"), &[]).unwrap();
        fs::write(root.join("sub/in.md"), "inside\n").unwrap();
        fs::write(root.join("above.md"), "above\n").unwrap();
        let after = snapshot(&root.join("sub"), &[]).unwrap();
        assert_eq!(written(&before, &after), vec!["../above.md", "in.md"]);
    }

    #[test]
    fn a_rename_names_both_paths() {
        assert_eq!(porcelain_paths("R  new.md\0old.md\0?? x.md\0"), vec![
            "new.md", "old.md", "x.md"
        ]);
    }
}
