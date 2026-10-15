//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit and the staging that bring generated files to a merged tree, and
//! the record that says they have been brought.

use mockspace_manifest::kept_ours::KEPT_OURS_FILE;

use super::*;

/// Whether `.gitattributes` has no uncommitted change, staged or not, so a
/// follow-up commit that names it carries only what this run wrote into it.
/// Absent counts as clean, since the run is then the one to create it. Asked
/// before the run writes its line, because afterwards it always differs.
pub(crate) fn attributes_clean(repo_root: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["status", "--porcelain", "--untracked-files=all", "--", ".gitattributes"])
        .output()
        .is_ok_and(|o| o.status.success() && o.stdout.is_empty())
}

/// A git command in `repo_root` whose paths are taken as written (for the
/// commands that accept the flag, which `check-ignore` does not): a generated
/// file may be named with a `[` or a `*` in it, which a pathspec would read as
/// a pattern and match its neighbours.
fn git_ok(repo_root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("--literal-pathspecs")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whether git ignores `rel`: `check-ignore` exits 0 for ignored, 1 for not,
/// and anything else is an error that says nothing about the path. Run without
/// `--literal-pathspecs`, which it rejects (exit 128), and a path it is given
/// is a name and never a pattern here either way.
fn is_ignored(repo_root: &Path, rel: &str) -> Result<bool, String> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["check-ignore", "-q", "--", rel])
        .status()
        .map_err(|e| e.to_string())?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        other => Err(format!("git check-ignore {rel} exited {other:?}")),
    }
}

/// The paths that exist, are not ignored, and so can be staged. A path git
/// cannot be asked about is left out and said so, never read as not ignored.
fn stageable(repo_root: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| repo_root.join(p).exists())
        .filter(|p| {
            match is_ignored(repo_root, p) {
                Ok(ignored) => !ignored,
                Err(e) => {
                    eprintln!("warning: {e}; leaving {p} out");
                    false
                },
            }
        })
        .cloned()
        .collect()
}

/// The generated paths, repository-relative, plus `.gitattributes` when asked.
/// The regeneration made the generated files, so they are its to commit; the
/// attributes file is only its to commit if nobody had edited it first, see
/// [`attributes_clean`].
pub(crate) fn tracked_set(
    repo_root: &Path,
    generated: &[PathBuf],
    include_attributes: bool,
) -> Vec<String> {
    let mut rels: Vec<String> = generated
        .iter()
        .filter_map(|p| relative(repo_root, p))
        .collect();
    if include_attributes {
        rels.push(".gitattributes".to_string());
    }
    rels.sort();
    rels.dedup();
    stageable(repo_root, &rels)
}

/// What the run wrote into `.gitattributes`, to be committed or staged exactly
/// as written and only when nobody had edited the file first.
///
/// `Some(content)` is `attributes_clean` having held before the run wrote its
/// line, and the content is what the file held right after. The commit carries
/// that content, never the working copy of the moment it is built, so an edit
/// made in between is left where it is.
pub(crate) type Attributes<'a> = Option<&'a str>;

fn pinned(attributes: Attributes<'_>) -> Vec<(&str, &str)> {
    attributes
        .map(|c| (".gitattributes", c))
        .into_iter()
        .collect()
}

/// Stage the generated files, so the commit being made carries them.
pub(crate) fn stage(repo_root: &Path, generated: &[PathBuf], attributes: Attributes<'_>) -> bool {
    let paths = tracked_set(repo_root, generated, attributes.is_some());
    let (pins, rest): (Vec<&String>, Vec<&String>) =
        paths.iter().partition(|p| p.as_str() == ".gitattributes");
    if !rest.is_empty() {
        let mut args = vec!["add", "--"];
        args.extend(rest.iter().map(|p| p.as_str()));
        if !git_ok(repo_root, &args) {
            return false;
        }
    }
    match (pins.first(), attributes) {
        (Some(rel), Some(content)) => crate::plumbing::pin(repo_root, rel, content, None).is_ok(),
        _ => true,
    }
}

/// Where the generated files differ from `HEAD`, commit exactly those paths and
/// nothing else that may be staged. Quiet when nothing differs.
///
/// By plumbing and not `git commit`: `post-merge` runs while the merge state is
/// still in place, and git refuses a partial commit then (and a plain one would
/// conclude the merge a second time). The commit is on top of the merge commit,
/// and the real index is brought to it for these paths afterwards.
pub(crate) fn commit_follow_up(
    repo_root: &Path,
    generated: &[PathBuf],
    subject: &str,
    attributes: Attributes<'_>,
) -> bool {
    let paths = tracked_set(repo_root, generated, attributes.is_some());
    if paths.is_empty() {
        return true;
    }
    match crate::plumbing::commit_paths(repo_root, &paths, subject, true, &pinned(attributes)) {
        Ok(crate::plumbing::Landed::Unchanged) => true,
        Ok(crate::plumbing::Landed::Committed(_)) => {
            crate::plumbing::sync_index_to_head(repo_root, &paths).is_ok()
        },
        Err(e) => {
            eprintln!("error: {e}");
            false
        },
    }
}

/// What the end of a generation run does about git: attach the documents
/// directory to the merge driver, then bring the generated files to the tree
/// the way the caller asked, in a follow-up commit with `commit` as its subject
/// or into the index with `stage`, and clear the record of kept sides once that
/// has been looked at. Neither flag: the attribute is written and nothing else.
pub(crate) fn finish_generation(
    repo_root: &Path,
    docs_dir: &Path,
    generated: &[PathBuf],
    commit: Option<&str>,
    stage_it: bool,
) -> Result<(), &'static str> {
    // Asked first: afterwards the file differs from HEAD by our line, whoever
    // else had edited it, and an edit of somebody's own is theirs to commit.
    let was_clean = attributes_clean(repo_root);
    if write_gitattributes(repo_root, docs_dir) {
        eprintln!("  wrote the merge attribute for the documents directory");
    }
    let written = was_clean
        .then(|| fs::read_to_string(repo_root.join(".gitattributes")).ok())
        .flatten();
    let attributes = written.as_deref();
    if let Some(subject) = commit {
        if !commit_follow_up(repo_root, generated, subject, attributes) {
            return Err("the regenerated files could not be committed");
        }
        clear_kept_ours(repo_root, generated, true);
    } else if stage_it {
        if !stage(repo_root, generated, attributes) {
            return Err("the regenerated files could not be staged");
        }
        clear_kept_ours(repo_root, generated, false);
    }
    Ok(())
}

/// Where the record of kept sides is, for this working tree.
fn kept_ours_path(repo_root: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--git-path", KEPT_OURS_FILE])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    Some(if path.is_absolute() { path } else { repo_root.join(path) })
}

/// Remove the record the merge driver left, once the generated files are
/// brought to the tree and that has been looked at.
///
/// Looked at by asking git, not by trusting the run: `git status --porcelain`
/// over the generated paths must show nothing left uncommitted after a commit
/// (`committed`), or nothing left unstaged after a staging. The hook that
/// called this only knows its engine exited; an engine that ignored the flag
/// and exited 0 never gets here, and the record stands. The attributes file is
/// not among the paths: it is not a document, and an edit of somebody's own to
/// it says nothing about whether the documents match.
pub(crate) fn clear_kept_ours(repo_root: &Path, generated: &[PathBuf], committed: bool) {
    let Some(record) = kept_ours_path(repo_root) else {
        return;
    };
    if !record.exists() {
        return;
    }
    let paths = tracked_set(repo_root, generated, false);
    let verified = paths.is_empty() || {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(repo_root)
            .args(["--literal-pathspecs", "status", "--porcelain", "--"])
            .args(&paths);
        cmd.output().is_ok_and(|o| {
            o.status.success()
                && String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .all(|l| !committed && l.as_bytes().get(1) == Some(&b' '))
        })
    };
    if verified {
        let _ = fs::remove_file(record);
    } else {
        eprintln!(
            "warning: generated files still differ from the {}; leaving the record of kept sides",
            if committed { "commit" } else { "index" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ignored_path_is_not_stageable_and_a_tracked_one_is() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "dev"]);
        fs::write(root.join(".gitignore"), "*.svg\n").unwrap();
        fs::write(root.join("a.svg"), "x\n").unwrap();
        fs::write(root.join("b.md"), "x\n").unwrap();
        let paths = ["a.svg".to_string(), "b.md".to_string()];
        assert_eq!(stageable(root, &paths), ["b.md"]);
    }

    #[test]
    fn a_path_git_cannot_be_asked_about_is_left_out() {
        // not a repository: `check-ignore` fails with 128, which says nothing
        // about the path and must not read as "not ignored"
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "x\n").unwrap();
        assert!(is_ignored(dir.path(), "a.md").is_err());
        assert!(stageable(dir.path(), &["a.md".to_string()]).is_empty());
    }

    #[test]
    fn the_attributes_file_is_clean_when_absent_or_unchanged_and_not_when_edited() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@e.c"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "dev"]);
        git(&["commit", "-q", "--allow-empty", "-m", "base"]);
        assert!(attributes_clean(root), "absent");
        fs::write(root.join(".gitattributes"), "*.png binary\n").unwrap();
        assert!(!attributes_clean(root), "untracked, so somebody's own file");
        git(&["add", ".gitattributes"]);
        assert!(!attributes_clean(root), "staged and not committed");
        git(&["commit", "-q", "-m", "attrs"]);
        assert!(attributes_clean(root), "committed and unchanged");
        fs::write(root.join(".gitattributes"), "*.png binary\n*.bin binary\n").unwrap();
        assert!(!attributes_clean(root), "edited");
        assert_eq!(
            tracked_set(root, &[], false),
            Vec::<String>::new(),
            "an edited attributes file is not named when it is not asked for"
        );
        assert_eq!(tracked_set(root, &[], true), [".gitattributes"]);
    }

    #[test]
    fn staging_takes_a_path_literally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        git(&["init", "-q", "-b", "dev"]);
        fs::write(root.join("a[1].md"), "literal\n").unwrap();
        fs::write(root.join("a1.md"), "lookalike\n").unwrap();
        // `a[1].md` as a pattern also matches `a1.md`, which is not generated
        assert!(stage(root, &[root.join("a[1].md")], None));
        let staged = git(&["diff", "--cached", "--name-only"]);
        assert_eq!(staged.lines().collect::<Vec<_>>(), ["a[1].md"], "{staged}");
    }

    /// A repository with `docs/A.md` committed and a record of kept sides.
    fn repo_with_record() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@e.c"])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "dev"]);
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/A.md"), "one\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "base"]);
        fs::write(root.join(".git/mockspace-kept-ours"), "0000\ndocs/A.md\n").unwrap();
        let doc = root.join("docs/A.md");
        (dir, root, doc)
    }

    #[test]
    fn the_record_goes_when_the_documents_match_the_commit() {
        let (_dir, root, doc) = repo_with_record();
        clear_kept_ours(&root, &[doc], true);
        assert!(!root.join(".git/mockspace-kept-ours").exists());
    }

    #[test]
    fn the_record_stays_when_a_document_is_left_uncommitted() {
        let (_dir, root, doc) = repo_with_record();
        fs::write(&doc, "two\n").unwrap();
        clear_kept_ours(&root, &[doc], true);
        assert!(root.join(".git/mockspace-kept-ours").exists());
    }

    #[test]
    fn staging_clears_the_record_and_leaving_it_unstaged_does_not() {
        let (_dir, root, doc) = repo_with_record();
        fs::write(&doc, "two\n").unwrap();
        clear_kept_ours(&root, &[doc.clone()], false);
        assert!(root.join(".git/mockspace-kept-ours").exists(), "unstaged");
        assert!(stage(&root, &[doc.clone()], None));
        clear_kept_ours(&root, &[doc], false);
        assert!(!root.join(".git/mockspace-kept-ours").exists(), "staged");
    }

    #[test]
    fn a_pinned_attributes_file_is_committed_as_written_and_not_as_edited_since() {
        let (_dir, root, doc) = repo_with_record();
        fs::write(
            root.join(".gitattributes"),
            "/docs/* merge=x\n*.bin binary\n",
        )
        .unwrap();
        assert!(commit_follow_up(
            &root,
            &[doc],
            "chore: follow up",
            Some("/docs/* merge=x\n")
        ));
        let shown = Command::new("git")
            .args(["show", "HEAD:.gitattributes"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&shown.stdout), "/docs/* merge=x\n");
    }
}
