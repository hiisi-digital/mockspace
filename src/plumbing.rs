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

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Put `content` into the index at `rel` as a blob of its own, whatever the
/// working-tree copy holds by now. The entry is a regular file; `index` is the
/// temporary index, or the real one for `None`.
pub(crate) fn pin(
    root: &Path,
    rel: &str,
    content: &str,
    index: Option<&Path>,
) -> Result<(), String> {
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let blob = String::from_utf8_lossy(&out.stdout).trim().to_string();
    run(
        root,
        &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{rel}")],
        index,
    )
    .map(|_| ())
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
///
/// A path in `pinned` is committed with exactly the content given, and not
/// whatever the working tree holds when the index is built: the caller decided
/// what it wrote, and a later edit of somebody's is theirs.
pub(crate) fn commit_paths(
    root: &Path,
    paths: &[String],
    message: &str,
    skip_if_unchanged: bool,
    pinned: &[(&str, &str)],
) -> Result<Landed, String> {
    let index = temporary_index(root)?;
    let _ = std::fs::remove_file(&index);
    let result = build(root, paths, message, skip_if_unchanged, pinned, &index);
    let _ = std::fs::remove_file(&index);
    result
}

fn build(
    root: &Path,
    paths: &[String],
    message: &str,
    skip_if_unchanged: bool,
    pinned: &[(&str, &str)],
    index: &Path,
) -> Result<Landed, String> {
    let head = run(root, &["rev-parse", "HEAD"], None)?.trim().to_string();
    run(root, &["read-tree", "HEAD"], Some(index)).map_err(|e| format!("read-tree: {e}"))?;
    for rel in paths {
        if let Some((_, content)) = pinned.iter().find(|(p, _)| p == rel) {
            pin(root, rel, content, Some(index))?;
            continue;
        }
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
    let mut args = vec!["--literal-pathspecs", "reset", "-q", "--"];
    args.extend(paths.iter().map(String::as_str));
    run(root, &args, None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Two files whose names differ only where a glob would make them equal:
    /// `a[1].md` is the literal name, and as a pattern it also matches `a1.md`.
    fn repo_with_lookalikes() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "dev"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("a[1].md"), "literal\n").unwrap();
        std::fs::write(root.join("a1.md"), "lookalike\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "--no-verify", "-m", "chore: base"]);
        dir
    }

    #[test]
    fn syncing_the_index_takes_a_path_literally() {
        let dir = repo_with_lookalikes();
        let root = dir.path();
        std::fs::write(root.join("a[1].md"), "literal, edited\n").unwrap();
        std::fs::write(root.join("a1.md"), "lookalike, staged\n").unwrap();
        git(root, &["add", "a1.md"]);
        sync_index_to_head(root, &["a[1].md".to_string()]).unwrap();
        assert_eq!(
            git(root, &["status", "--porcelain"]),
            "M  a1.md\n M a[1].md\n",
            "the staged lookalike is somebody else's and stays staged"
        );
    }

    #[test]
    fn committing_paths_takes_a_path_literally() {
        let dir = repo_with_lookalikes();
        let root = dir.path();
        std::fs::write(root.join("a[1].md"), "literal, edited\n").unwrap();
        std::fs::write(root.join("a1.md"), "lookalike, edited\n").unwrap();
        let landed = commit_paths(root, &["a[1].md".to_string()], "chore: one", true, &[]).unwrap();
        assert!(matches!(landed, Landed::Committed(_)));
        assert_eq!(git(root, &["show", "HEAD:a1.md"]), "lookalike\n");
        assert_eq!(git(root, &["show", "HEAD:a[1].md"]), "literal, edited\n");
    }

    #[test]
    fn a_pinned_path_is_committed_as_given_whatever_the_working_tree_holds_now() {
        let dir = repo_with_lookalikes();
        let root = dir.path();
        std::fs::write(root.join("a1.md"), "lookalike, as the run wrote it\n").unwrap();
        // somebody edits it after the run decided what to commit
        std::fs::write(
            root.join("a1.md"),
            "lookalike, and then somebody's own line\n",
        )
        .unwrap();
        let landed = commit_paths(root, &["a1.md".to_string()], "chore: pinned", true, &[(
            "a1.md",
            "lookalike, as the run wrote it\n",
        )])
        .unwrap();
        assert!(matches!(landed, Landed::Committed(_)));
        assert_eq!(
            git(root, &["show", "HEAD:a1.md"]),
            "lookalike, as the run wrote it\n"
        );
        sync_index_to_head(root, &["a1.md".to_string()]).unwrap();
        assert_eq!(
            git(root, &["status", "--porcelain"]),
            " M a1.md\n",
            "their edit is still theirs, uncommitted"
        );
    }
}
