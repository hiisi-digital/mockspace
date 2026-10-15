//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A commit of exactly some paths, built beside the real index.
//!
//! The commit goes through a temporary index, so it never reads or writes the
//! real one: everything somebody has staged stays staged, and nothing is
//! committed that was not asked for. It also needs no hook to have passed and
//! is not signed, which is the point where `git commit` cannot be used at all,
//! such as the `post-merge` hook, where the merge state is still in place and
//! git refuses a partial commit.
//!
//! The real index then still holds the old layout for those paths, so the
//! caller brings them to the new `HEAD` with [`sync_index_to_head`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a call landed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Landed {
    /// A commit was made, with this id.
    Committed(String),
    /// Nothing differed from `HEAD` for those paths, so nothing was made.
    Unchanged,
}

fn run(root: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(root);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        String::from_utf8(out.stdout).map_err(|e| e.to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Where the temporary index lives: the repository's own git directory, which
/// is not `root/.git` in a linked worktree.
fn temporary_index(root: &Path) -> Result<PathBuf, String> {
    let path = run(
        root,
        &["rev-parse", "--git-path", "tmp_mockspace_index"],
        None,
    )?;
    let path = PathBuf::from(path.trim());
    Ok(if path.is_absolute() { path } else { root.join(path) })
}

/// Commit exactly `paths` (relative to `root`) on top of `HEAD`, on the branch
/// `HEAD` names. A path that no longer exists is recorded as removed.
///
/// With `skip_if_unchanged`, a call whose paths already match `HEAD` makes no
/// commit.
pub(crate) fn commit_paths(
    root: &Path,
    paths: &[String],
    message: &str,
    skip_if_unchanged: bool,
) -> Result<Landed, String> {
    let index = temporary_index(root)?;
    let _ = std::fs::remove_file(&index);
    let result = build(root, paths, message, skip_if_unchanged, &index);
    let _ = std::fs::remove_file(&index);
    result
}

fn build(
    root: &Path,
    paths: &[String],
    message: &str,
    skip_if_unchanged: bool,
    index: &Path,
) -> Result<Landed, String> {
    let head = run(root, &["rev-parse", "HEAD"], None)?.trim().to_string();
    run(root, &["read-tree", "HEAD"], Some(index)).map_err(|e| format!("read-tree: {e}"))?;
    for rel in paths {
        let flag = if root.join(rel).exists() { "--add" } else { "--remove" };
        run(root, &["update-index", flag, "--", rel], Some(index))
            .map_err(|e| format!("update-index {rel}: {e}"))?;
    }
    if skip_if_unchanged
        && run(
            root,
            &["diff-index", "--cached", "--quiet", "HEAD"],
            Some(index),
        )
        .is_ok()
    {
        return Ok(Landed::Unchanged);
    }
    let tree = run(root, &["write-tree"], Some(index)).map_err(|e| format!("write-tree: {e}"))?;
    let commit = run(
        root,
        &["commit-tree", tree.trim(), "-p", &head, "-m", message],
        None,
    )
    .map_err(|e| format!("commit-tree: {e}"))?
    .trim()
    .to_string();
    let branch = run(root, &["symbolic-ref", "--short", "HEAD"], None)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "HEAD".to_string());
    let reference = if branch == "HEAD" {
        "HEAD".to_string()
    } else {
        format!("refs/heads/{branch}")
    };
    run(root, &["update-ref", &reference, &commit, &head], None)
        .map_err(|e| format!("update-ref (commit object {commit}): {e}"))?;
    Ok(Landed::Committed(commit))
}

/// Set the real index entries for `paths` to what `HEAD` holds, touching no
/// working-tree file. A path `HEAD` does not have, the old half of a rename, is
/// dropped from the index. Path-limited, so anything staged elsewhere stays.
pub(crate) fn sync_index_to_head(root: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "-q", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(root, &args, None).map(|_| ())
}
