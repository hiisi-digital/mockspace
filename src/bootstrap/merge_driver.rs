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
pub(crate) fn pattern(rel: &str) -> String {
    let mut out = String::from("/");
    for c in rel.chars() {
        if matches!(c, ' ' | '#' | '!' | '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub(crate) fn relative(repo_root: &Path, path: &Path) -> Option<String> {
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
    with_note_line(existing, NOTE, line)
}

/// The same with the note that introduces `line`.
pub(crate) fn with_note_line(existing: &str, note: &str, line: &str) -> String {
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
    text.push_str(note);
    text.push('\n');
    text.push_str(line);
    text.push('\n');
    text
}

/// Write the attribute lines into the repository's `.gitattributes`: the documents
/// directory always, the registry directory once it exists, and the test-history
/// directory always: its first file is written by a test run on any branch, and the
/// line is the same text whether or not that has happened yet. The design rounds
/// directory is always written too, for the same reason, and so are the append-only
/// files' lines, each naming one file or one narrow pattern.
/// Returns whether the file changed.
pub(crate) fn write_gitattributes(
    repo_root: &Path,
    docs_dir: &Path,
    registry_dir: &Path,
    history_dir: &Path,
    append_lines: &[String],
) -> bool {
    let Some(line) = attributes_line(repo_root, docs_dir) else {
        return false;
    };
    let path = repo_root.join(".gitattributes");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut next = with_attributes(&existing, &line);
    if registry_dir.is_dir() {
        if let Some(line) = registry_attributes_line(repo_root, registry_dir) {
            next = with_note_line(&next, REGISTRY_NOTE, &line);
        }
    }
    // Beside the registry, the design rounds: the line is written before the
    // first round exists, so it is there on every branch that will ever close one.
    let rounds_dir = registry_dir.with_file_name("design_rounds");
    if let Some(line) = round_attributes_line(repo_root, &rounds_dir) {
        next = with_note_line(&next, ROUND_NOTE, &line);
    }
    if let Some(line) = history_attributes_line(repo_root, history_dir) {
        next = with_note_line(&next, HISTORY_NOTE, &line);
    }
    next = with_lines(&next, APPEND_NOTE, append_lines);
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
}
