//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Generated files do not conflict, and the merged tree gets them regenerated.
//!
//! A conflict confined to a file mockspace generates is not a conflict: the file
//! is a function of the tree, so whichever side a merge takes it is wrong until
//! generation has run over the merged result. Stopping a merge on one only asks
//! somebody to run the generator by hand, for every agent, every time.
//!
//! Three parts. A merge driver, named in `.gitattributes` for each generated
//! path, resolves the file without a conflict by keeping ours. Generation then
//! runs over the merged tree, at the commit that concludes a merge that stopped
//! (the `pre-commit` hook, with `MERGE_HEAD` present) or, where the merge made
//! its own commit, in a follow-up commit (the `post-merge` hook), because git
//! does not carry index changes a `pre-merge-commit` hook makes into the merge
//! commit. The driver is registered in the clone's config, which is the only
//! place git accepts one from: a repository cannot carry its own.

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const DRIVER: &str = "mockspace-generated";

/// The subject of the commit that brings generated files to a merge commit.
pub(crate) const FOLLOW_UP_SUBJECT: &str = "chore: regenerate generated files after merge";

const BEGIN: &str = "# mockspace-generated: begin, rewritten by `mock`, do not edit";
const END: &str = "# mockspace-generated: end";

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

/// The managed block naming every generated path, sorted so the same set always
/// writes the same text.
pub(crate) fn attributes_block(repo_root: &Path, generated: &[PathBuf]) -> String {
    let mut rels: Vec<String> = generated
        .iter()
        .filter_map(|p| relative(repo_root, p))
        .collect();
    rels.sort();
    rels.dedup();
    let mut block = format!("{BEGIN}\n");
    for rel in rels {
        block.push_str(&format!("{} merge={DRIVER}\n", pattern(&rel)));
    }
    block.push_str(END);
    block.push('\n');
    block
}

/// `existing` with the managed block replaced by `block`, or appended when
/// there is none. Whatever else the file holds is left exactly as it was.
pub(crate) fn splice_block(existing: &str, block: &str) -> String {
    let start = existing.lines().position(|l| l == BEGIN);
    let end = existing.lines().position(|l| l == END);
    match (start, end) {
        (Some(s), Some(e)) if s <= e => {
            let mut out: Vec<&str> = existing.lines().take(s).collect();
            let tail: Vec<&str> = existing.lines().skip(e + 1).collect();
            let mut text = out.drain(..).collect::<Vec<_>>().join("\n");
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(block);
            if !tail.is_empty() {
                text.push_str(&tail.join("\n"));
                text.push('\n');
            }
            text
        },
        _ => {
            let mut text = existing.to_string();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(block);
            text
        },
    }
}

/// Write the managed block into the repository's `.gitattributes`. Returns
/// whether the file changed.
pub(crate) fn write_gitattributes(repo_root: &Path, generated: &[PathBuf]) -> bool {
    let path = repo_root.join(".gitattributes");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let next = splice_block(&existing, &attributes_block(repo_root, generated));
    if next == existing {
        return false;
    }
    fs::write(&path, next).is_ok()
}

/// Register the driver in the clone's config. Resolves a file by keeping ours
/// and reporting no conflict; generation runs over the merged tree afterwards,
/// so which side it kept never reaches a commit.
pub(crate) fn register_driver(repo_root: &Path, actions: &mut Vec<String>) {
    let set = |key: &str, value: &str| {
        Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["config", "--local", key, value])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    let current = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", &format!("merge.{DRIVER}.driver")])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if current.as_deref() == Some("true") {
        return;
    }
    if set(&format!("merge.{DRIVER}.name"), "mockspace generated file")
        && set(&format!("merge.{DRIVER}.driver"), "true")
    {
        actions.push(format!("registered the {DRIVER} merge driver"));
    } else {
        actions.push(format!("could not register the {DRIVER} merge driver"));
    }
}

fn git_ok(repo_root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The paths that exist, are not ignored, and so can be staged.
fn stageable(repo_root: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| repo_root.join(p).exists())
        .filter(|p| !git_ok(repo_root, &["check-ignore", "-q", "--", p]))
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
pub(crate) fn commit_follow_up(repo_root: &Path, generated: &[PathBuf]) -> bool {
    let paths = tracked_set(repo_root, generated);
    if paths.is_empty() {
        return true;
    }
    match crate::plumbing::commit_paths(repo_root, &paths, FOLLOW_UP_SUBJECT, true) {
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
    fn the_block_names_each_path_once_sorted_and_anchored() {
        let root = Path::new("/r");
        let block = attributes_block(root, &[
            root.join("docs/B.md"),
            root.join("docs/A.md"),
            root.join("docs/B.md"),
        ]);
        assert_eq!(
            block,
            format!("{BEGIN}\n/docs/A.md merge={DRIVER}\n/docs/B.md merge={DRIVER}\n{END}\n")
        );
    }

    #[test]
    fn a_path_that_reads_as_pattern_syntax_is_escaped() {
        assert_eq!(pattern("docs/a b#c.md"), "/docs/a\\ b\\#c.md");
    }

    #[test]
    fn splicing_replaces_only_the_block_and_keeps_the_rest() {
        let first = splice_block("*.png binary\n", "B1\n");
        assert!(first.starts_with("*.png binary\n"));
        let block1 = format!("{BEGIN}\nx\n{END}\n");
        let with = splice_block("*.png binary\n", &block1);
        let block2 = format!("{BEGIN}\ny\n{END}\n");
        let swapped = splice_block(&format!("{with}*.lock -diff\n"), &block2);
        assert!(swapped.contains("*.png binary\n"), "{swapped}");
        assert!(swapped.contains("*.lock -diff\n"), "{swapped}");
        assert!(swapped.contains("\ny\n"), "{swapped}");
        assert!(!swapped.contains("\nx\n"), "{swapped}");
        assert_eq!(swapped.matches(BEGIN).count(), 1);
    }

    #[test]
    fn splicing_the_same_block_twice_changes_nothing() {
        let block = format!("{BEGIN}\nx\n{END}\n");
        let once = splice_block("a\n", &block);
        assert_eq!(splice_block(&once, &block), once);
    }
}
