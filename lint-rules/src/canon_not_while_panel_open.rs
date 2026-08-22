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
        .args(["diff", "--cached", "--name-only", "-z"])
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
        let Some(hits) = canon_violation(&staged, ctx.canon_paths, true) else {
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
