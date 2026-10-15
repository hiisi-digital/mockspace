//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Generated files do not conflict in a merge, and the merged tree gets them regenerated.
//!
//! A conflict confined to a file mockspace generates is not a conflict: the file
//! is a function of the tree, so whichever side a merge takes it is wrong until
//! generation has run over the merged result. Stopping a merge on one only asks
//! somebody to run the generator by hand, for every agent, every time.
//!
//! Three parts. A merge driver, `mockspace-generated`, is attached by a
//! `.gitattributes` line that names the documents directory and nothing else, so
//! it is the same text on every branch and cannot conflict itself (a line per
//! generated path did: two branches each adding a document rewrote adjacent
//! lines). The directory also holds files nobody generates, so the driver is
//! what tells them apart: it reads the file. Ours is kept when it carries the
//! generation header, and anything else is merged by `git merge-file`, which is
//! what git does with no driver.
//!
//! The driver keeps ours only where a regeneration is certain to follow, which
//! is the two flows below and nothing else. After a `git merge` it is at the
//! commit that concludes a merge that stopped (the `pre-commit` hook, with
//! `MERGE_HEAD` or `SQUASH_MSG` present) or, where the merge made its own
//! commit, in a follow-up commit (the `post-merge` hook), because git does not
//! carry index changes a `pre-merge-commit` hook makes into the merge commit.
//! After a rebase it is a follow-up commit on the rewritten tip (the
//! `post-rewrite` hook): the commits in between may carry the upstream's copy,
//! the tip does not. A cherry-pick, a revert, `git am`, `git checkout -m` and a
//! stash application run no hook of ours, and a driver keeping ours there drops
//! the other side's edit with nothing to put it back, so there it merges as git
//! would. So does a clone whose hooks do not run: the driver asks git where the
//! hook is (`core.hooksPath` moves it) and keeps ours only if it is there and
//! ours. See [`super::driver_script`].
//!
//! The driver is registered in the clone's config, which is the only place git
//! accepts one from: a repository cannot carry its own.

use super::driver_script::driver_script;
use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const DRIVER: &str = "mockspace-generated";

/// The subject of the commit that brings generated files to a merge commit.
pub(crate) const FOLLOW_UP_SUBJECT: &str = "chore: regenerate generated files after merge";

/// The subject of the commit that brings generated files to a rebased tip.
pub(crate) const FOLLOW_UP_SUBJECT_REBASE: &str = "chore: regenerate generated files after rebase";

const NOTE: &str = "# mockspace: generated documents merge by regeneration, see the merge driver";

/// A repository-relative path as a `.gitattributes` pattern: `/` separated,
/// anchored at the root, and with what a pattern reads as syntax escaped.
fn pattern(rel: &str) -> String {
    let mut out = String::from("/");
    for c in rel.chars() {
        if matches!(c, ' ' | '#' | '!' | '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn relative(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// The one attribute line: every top-level file of the documents directory goes
/// to the driver. Subdirectories are not matched, which is right, since nothing
/// generated lands in one. `None` when the directory is outside the repository.
pub(crate) fn attributes_line(repo_root: &Path, docs_dir: &Path) -> Option<String> {
    let rel = relative(repo_root, docs_dir).filter(|r| !r.is_empty())?;
    Some(format!("{}/* merge={DRIVER}", pattern(&rel)))
}

/// `existing` with the note and `line` at the end, unless `line` is already
/// there. Whatever else the file holds is left exactly as it was.
pub(crate) fn with_attributes(existing: &str, line: &str) -> String {
    if existing.lines().any(|l| l == line) {
        return existing.to_string();
    }
    let mut text = existing.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(NOTE);
    text.push('\n');
    text.push_str(line);
    text.push('\n');
    text
}

/// Write the driver's attribute line into the repository's `.gitattributes`.
/// Returns whether the file changed.
pub(crate) fn write_gitattributes(repo_root: &Path, docs_dir: &Path) -> bool {
    let Some(line) = attributes_line(repo_root, docs_dir) else {
        return false;
    };
    let path = repo_root.join(".gitattributes");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let next = with_attributes(&existing, &line);
    if next == existing {
        return false;
    }
    fs::write(&path, next).is_ok()
}

/// Where the script lives: the clone's git directory, shared by its worktrees.
fn driver_script_path(repo_root: &Path) -> PathBuf {
    resolve_git_dir(repo_root)
        .join("mockspace")
        .join("merge-generated")
}

/// Write the script, executable. Returns whether it changed.
fn ensure_driver_script(path: &Path) -> bool {
    let script = driver_script();
    if fs::read_to_string(path).is_ok_and(|current| current == script) {
        return false;
    }
    let written = path
        .parent()
        .is_some_and(|dir| fs::create_dir_all(dir).is_ok())
        && fs::write(path, script).is_ok();
    #[cfg(unix)]
    if written {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
    written
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// Register the driver in the clone's config, pointing at the script.
pub(crate) fn register_driver(repo_root: &Path, actions: &mut Vec<String>) {
    let script = driver_script_path(repo_root);
    let wanted = format!("{} %O %A %B %P %L", quoted(&script));
    let key = format!("merge.{DRIVER}.driver");
    let current = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", &key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let wrote_script = ensure_driver_script(&script);
    if current.as_deref() == Some(wanted.as_str()) {
        if wrote_script {
            actions.push(format!("rewrote the {DRIVER} merge driver"));
        }
        return;
    }
    let set = |key: &str, value: &str| {
        Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["config", "--local", key, value])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    if script.is_file()
        && set(&format!("merge.{DRIVER}.name"), "mockspace generated file")
        && set(&key, &wanted)
    {
        actions.push(format!("registered the {DRIVER} merge driver"));
    } else {
        actions.push(format!("could not register the {DRIVER} merge driver"));
    }
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

/// The generated paths plus `.gitattributes`, repository-relative.
pub(crate) fn tracked_set(repo_root: &Path, generated: &[PathBuf]) -> Vec<String> {
    let mut rels: Vec<String> = generated
        .iter()
        .filter_map(|p| relative(repo_root, p))
        .collect();
    rels.push(".gitattributes".to_string());
    rels.sort();
    rels.dedup();
    stageable(repo_root, &rels)
}

/// Stage the generated files, so the commit being made carries them.
pub(crate) fn stage(repo_root: &Path, generated: &[PathBuf]) -> bool {
    let paths = tracked_set(repo_root, generated);
    if paths.is_empty() {
        return true;
    }
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_ok(repo_root, &args)
}

/// Where the generated files differ from `HEAD`, commit exactly those paths and
/// nothing else that may be staged. Quiet when nothing differs.
///
/// By plumbing and not `git commit`: `post-merge` runs while the merge state is
/// still in place, and git refuses a partial commit then (and a plain one would
/// conclude the merge a second time). The commit is on top of the merge commit,
/// and the real index is brought to it for these paths afterwards.
pub(crate) fn commit_follow_up(repo_root: &Path, generated: &[PathBuf], subject: &str) -> bool {
    let paths = tracked_set(repo_root, generated);
    if paths.is_empty() {
        return true;
    }
    match crate::plumbing::commit_paths(repo_root, &paths, subject, true) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_documents_directory_once_and_no_file() {
        let root = Path::new("/r");
        assert_eq!(
            attributes_line(root, &root.join("docs")).unwrap(),
            format!("/docs/* merge={DRIVER}")
        );
        assert_eq!(attributes_line(root, Path::new("/elsewhere/docs")), None);
        assert_eq!(attributes_line(root, root), None);
    }

    #[test]
    fn a_path_that_reads_as_pattern_syntax_is_escaped() {
        assert_eq!(pattern("docs/a b#c.md"), "/docs/a\\ b\\#c.md");
    }

    #[test]
    fn adding_the_line_keeps_the_rest_and_adds_it_once() {
        let line = format!("/docs/* merge={DRIVER}");
        let once = with_attributes("*.png binary\n*.lock -diff", &line);
        assert!(once.starts_with("*.png binary\n*.lock -diff\n"), "{once}");
        assert_eq!(once.matches(&line).count(), 1, "{once}");
        assert_eq!(
            with_attributes(&once, &line),
            once,
            "a second pass changes nothing"
        );
    }

    #[test]
    fn adding_the_line_to_nothing_writes_only_the_note_and_the_line() {
        let line = format!("/docs/* merge={DRIVER}");
        assert_eq!(with_attributes("", &line), format!("{NOTE}\n{line}\n"));
    }

    #[test]
    fn the_driver_script_is_valid_shell_and_names_its_arguments() {
        let script = driver_script();
        let file = std::env::temp_dir().join(format!("ms_driver_{}.sh", std::process::id()));
        fs::write(&file, &script).unwrap();
        let out = Command::new("sh").arg("-n").arg(&file).output().unwrap();
        let _ = fs::remove_file(&file);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(script.contains(crate::render_design::GENERATED_MARKER));
        assert!(script.contains("git merge-file"));
    }

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
        assert!(stage(root, &[root.join("a[1].md")]));
        let staged = git(&["diff", "--cached", "--name-only"]);
        assert_eq!(staged.lines().collect::<Vec<_>>(), ["a[1].md"], "{staged}");
    }
}
