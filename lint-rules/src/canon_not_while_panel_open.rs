//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Canon is not written while a panel is open.
//!
//! A panel argues, converges and proposes; what it produces is a proposal, and
//! somebody else admits it. The failure this guards is a panel writing straight
//! into the canon, which is how a canon fills with the argument that produced
//! it rather than the answer.
//!
//! **A lint rather than a readiness row, because the gate is the commit.** The
//! generated pre-commit hook already runs `mock --lint-only`, which skips the
//! build and runs the lint pass, so this costs nothing new to install and no
//! second copy of the rule in generated bash.
//!
//! **It reads the staged blob, not the worktree.** What gets committed is what
//! is staged, so judging the worktree flags an edit the commit does not carry
//! and misses one it does. `changelist_required` reached the same conclusion
//! for the same reason and its note is worth reading.
//!
//! Silent where the project declares no canon paths: a project that has not
//! said what its canon is has declared nothing to protect, and a row about an
//! unconfigured feature on every run is noise nobody asked for.

use std::path::Path;
use std::process::Command;

use crate::path_filter::glob_match;
use crate::{Lint, LintError, RepoContext, RepoLint, Severity};

pub struct CanonNotWhilePanelOpen;

impl Lint for CanonNotWhilePanelOpen {
    fn name(&self) -> &'static str {
        "canon-not-while-panel-open"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

/// Every path the index holds against HEAD, repo-root-relative.
///
/// `-z` rather than the default: `git` C-quotes a path containing a non-ASCII
/// or special character under `core.quotePath`, so `mock/canon/lähde.md`
/// arrives as `"mock/canon/l\303\244hde.md"`. Stripping the quotes leaves the
/// escapes, which match no glob, so the check passes on exactly the paths a
/// Finnish-named project is most likely to have. `-z` emits the raw bytes with
/// a NUL terminator and no quoting at all.
pub fn staged_paths(repo_root: &Path) -> Option<Vec<String>> {
    let out = Command::new("git")
        .args(["diff", "--cached", "--name-only", "--no-renames", "-z"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Which of `changed` a panel being open forbids, or `None` where nothing is.
///
/// Shared with the readiness report, which asks the same question of the
/// working tree. The two surfaces are different on purpose: a commit is judged
/// on what it stages, and a person asking whether the repository is ready is
/// asking about what is in front of them.
#[must_use]
pub fn canon_violation(
    changed: &[String],
    canon_paths: &[String],
    any_panel_open: bool,
) -> Option<Vec<String>> {
    if !any_panel_open || canon_paths.is_empty() {
        return None;
    }
    let hits: Vec<String> = changed
        .iter()
        .filter(|p| canon_paths.iter().any(|g| glob_match(g, p)))
        .cloned()
        .collect();
    (!hits.is_empty()).then_some(hits)
}

/// The canon paths in `staged` that `inherited` does not excuse, or `None`.
///
/// `inherited` is asked about the canon hits only, and in their order, after
/// [`canon_violation`] has chosen them. It is a closure so that the test can
/// see which paths it was asked about.
fn authored_canon(
    staged: &[String],
    canon_paths: &[String],
    mut inherited: impl FnMut(&str) -> bool,
) -> Option<Vec<String>> {
    let hits = canon_violation(staged, canon_paths, true)?;
    let authored: Vec<String> = hits.into_iter().filter(|file| !inherited(file)).collect();
    (!authored.is_empty()).then_some(authored)
}

impl RepoLint for CanonNotWhilePanelOpen {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        if ctx.canon_paths.is_empty() || ctx.open_panels.is_empty() {
            return Vec::new();
        }
        // A git call that fails is not a commit with nothing staged. Reporting
        // it as clean would be a positive assertion this did not establish,
        // which is the failure the readiness report's own error arm had.
        let Some(staged) = staged_paths(ctx.repo_root) else {
            return vec![LintError::with_severity(
                "unknown".to_string(),
                0,
                "canon-not-while-panel-open",
                "could not read the index, so this checked nothing. A check that did not run \
                 is not a check that passed."
                    .to_string(),
                Severity::ADVISORY,
            )];
        };
        // A merge brings in canon somebody else admitted, and a recombined
        // conflict in it authors nothing: the panel did not write those
        // lines. Only what the merge itself did not bring in is the panel's.
        // The merge filter runs on the canon hits alone, since it spawns git
        // for every path it is asked about and the rest of the index is none
        // of this lint's business.
        let merge = crate::merge::Merge::detect(ctx.repo_root);
        let in_merge = merge.in_progress();
        let Some(hits) = authored_canon(&staged, ctx.canon_paths, |file| {
            in_merge && merge.inherited(ctx.repo_root, file)
        }) else {
            return Vec::new();
        };
        vec![LintError::error(
            "unknown".to_string(),
            0,
            "canon-not-while-panel-open",
            format!(
                "canon is staged while {} is open: {}. A panel proposes; somebody else admits. \
                 Consolidate the panel, or make this edit outside it.",
                ctx.open_panels.join(", "),
                hits.join(", ")
            ),
        )]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::process::Command;

    use super::*;

    const CANON: &str = "mock/canon/a.md";

    fn git(root: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"))
            .status
            .success()
    }

    fn must(root: &Path, args: &[&str]) {
        assert!(git(root, args), "git {args:?} failed");
    }

    fn write(root: &Path, body: &str) {
        std::fs::create_dir_all(root.join("mock/canon")).unwrap();
        std::fs::write(root.join(CANON), body).unwrap();
    }

    fn init() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        must(tmp.path(), &["init", "-q", "-b", "trunk"]);
        must(tmp.path(), &["config", "user.email", "t@example.com"]);
        must(tmp.path(), &["config", "user.name", "t"]);
        must(tmp.path(), &["config", "commit.gpgsign", "false"]);
        tmp
    }

    /// A merge in flight over a canon file both sides edited on one line.
    fn conflicted() -> tempfile::TempDir {
        let tmp = init();
        let root = tmp.path();
        write(root, "one\ntwo\nthree\n");
        must(root, &["add", "-A"]);
        must(root, &["commit", "-q", "-m", "base"]);
        must(root, &["switch", "-q", "-c", "feature"]);
        write(root, "one\nBRANCH\nthree\n");
        must(root, &["commit", "-q", "-am", "branch"]);
        must(root, &["switch", "-q", "trunk"]);
        write(root, "one\nTRUNK\nthree\n");
        must(root, &["commit", "-q", "-am", "trunk"]);
        must(root, &["switch", "-q", "feature"]);
        assert!(
            !git(root, &["merge", "--no-commit", "--no-ff", "trunk"]),
            "the fixture was meant to conflict"
        );
        tmp
    }

    fn findings(root: &Path) -> Vec<LintError> {
        let (crates, canon, panels) = (
            BTreeSet::new(),
            vec!["mock/canon/**".to_string()],
            vec!["a-panel".to_string()],
        );
        CanonNotWhilePanelOpen.check_repo(&RepoContext {
            mock_dir:    &root.join("mock"),
            repo_root:   root,
            all_crates:  &crates,
            src_dirs:    &[],
            invocation:  None,
            canon_paths: &canon,
            open_panels: &panels,
            registry:    &Default::default(),
        })
    }

    #[test]
    fn canon_recombined_from_both_sides_in_a_merge_is_not_the_panels() {
        let tmp = conflicted();
        write(tmp.path(), "one\nBRANCH\nTRUNK\nthree\n");
        must(tmp.path(), &["add", CANON]);
        assert!(findings(tmp.path()).is_empty());
    }

    #[test]
    fn canon_with_a_line_smuggled_into_a_merge_is_still_refused() {
        let tmp = conflicted();
        write(tmp.path(), "one\nBRANCH\nTRUNK\nSMUGGLED\nthree\n");
        must(tmp.path(), &["add", CANON]);
        let found = findings(tmp.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains(CANON), "{}", found[0].message);
    }

    #[test]
    fn the_merge_filter_is_asked_about_canon_paths_only() {
        let staged: Vec<String> = ["src/lib.rs", CANON, "docs/a.md", "mock/canon/b.md"]
            .map(String::from)
            .to_vec();
        let canon = vec!["mock/canon/**".to_string()];
        let mut asked: Vec<String> = Vec::new();
        let authored = authored_canon(&staged, &canon, |file| {
            asked.push(file.to_string());
            file == CANON
        });
        assert_eq!(asked, ["mock/canon/a.md", "mock/canon/b.md"]);
        assert_eq!(authored, Some(vec!["mock/canon/b.md".to_string()]));
    }

    #[test]
    fn canon_the_merge_brought_in_leaves_nothing_to_refuse() {
        let staged = vec![CANON.to_string(), "src/lib.rs".to_string()];
        let canon = vec!["mock/canon/**".to_string()];
        assert_eq!(authored_canon(&staged, &canon, |_| true), None);
        assert_eq!(
            authored_canon(&staged, &canon, |_| false),
            Some(vec![CANON.to_string()])
        );
    }

    /// A canon file moved to a path outside the canon is a change to the canon.
    /// With git's rename detection on, the staged listing names the destination
    /// alone, which is outside every canon glob, and the panel is not stopped.
    #[test]
    fn canon_moved_out_of_the_canon_is_still_a_canon_change() {
        let tmp = init();
        let root = tmp.path();
        write(root, "one\ntwo\nthree\nfour\nfive\n");
        must(root, &["add", "-A"]);
        must(root, &["commit", "-q", "-m", "base"]);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        must(root, &["mv", CANON, "docs/a.md"]);
        let staged = staged_paths(root).unwrap();
        assert!(staged.contains(&CANON.to_string()), "the source is a deletion: {staged:?}");
        assert!(!findings(root).is_empty(), "the move changed the canon");
    }

    #[test]
    fn the_same_edit_with_no_merge_in_flight_is_refused() {
        let tmp = init();
        write(tmp.path(), "one\n");
        must(tmp.path(), &["add", "-A"]);
        must(tmp.path(), &["commit", "-q", "-m", "base"]);
        write(tmp.path(), "one\ntwo\n");
        must(tmp.path(), &["add", CANON]);
        assert_eq!(findings(tmp.path()).len(), 1);
    }
}
