//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Cross-crate lint: block edits to frozen changelists.
//!
//! Frozen changelists cannot be modified:
//! - Locked doc CLs are frozen forever.
//! - Locked src CLs are frozen forever.
//! - Deprecated CLs are frozen forever.
//! - Active doc CL is editable only in DOC phase.
//! - Active src CL is editable only in IMPL phase.
//!
//! Detection: find all changelists in `design_rounds/`, check if any
//! appear in staged or unstaged changes, and validate against the
//! current phase.
//!
//! Severity: Error (blocks commit and push).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, ClKind, ClStatus, ParsedChangelist, Phase};
use crate::{Lint, RepoLint, LintError, RepoContext};

const LINT_NAME: &str = "changelist-immutability";

pub struct ChangelistImmutability;

impl Lint for ChangelistImmutability {
    fn name(&self) -> &'static str {
        LINT_NAME
    }
    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistImmutability {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        // The mock dir comes straight from the context. Previously this stole a
        // `workspace_root` from `crates.first()` and returned no findings when
        // there were no crates, which silently disabled the gate in a repo whose
        // taxonomy had not been settled yet.
        let workspace_root = ctx.mock_dir;

        let design_rounds = workspace_root.join("design_rounds");
        let all_cls = changelist_helpers::find_changelists(&design_rounds);
        if all_cls.is_empty() {
            return Vec::new();
        }

        let phase = changelist_helpers::current_phase(&design_rounds);

        let modified = get_modified_in_design_rounds(workspace_root);
        if modified.is_empty() {
            return Vec::new();
        }

        let mut errors = Vec::new();

        for cl in &all_cls {
            let cl_rel = format!("design_rounds/{}", cl.filename);

            // Is this changelist being modified?
            let source = match modified.iter().find(|(f, _)| f == &cl_rel) {
                Some((_, s)) => s.clone(),
                None => continue,
            };

            if let Some(msg) = check_changelist_edit(cl, phase) {
                errors.push(LintError::error(
                    "workspace".to_string(),
                    0,
                    LINT_NAME,
                    format!("changelist `{}` ({source}) {msg}", cl.filename,),
                ));
            }
        }

        errors
    }
}

/// Check if editing a changelist is forbidden given the current phase.
/// Returns an error message if the edit is blocked, None if allowed.
fn check_changelist_edit(cl: &ParsedChangelist, phase: Phase) -> Option<String> {
    match cl.status {
        ClStatus::Locked => {
            Some(format!(
                "cannot be modified: it is locked and frozen forever. \
                 Use SHAME.md.tmpl to document gaps discovered during execution."
            ))
        },
        ClStatus::Deprecated => {
            Some(format!(
                "cannot be modified: it is deprecated and frozen forever."
            ))
        },
        ClStatus::Active => {
            match cl.kind {
                ClKind::Doc => {
                    if phase != Phase::Doc {
                        Some(format!(
                            "cannot be modified in phase {}: active doc changelists \
                             are only editable in DOC phase.",
                            phase.label(),
                        ))
                    } else {
                        None // allowed
                    }
                },
                ClKind::Src => {
                    if phase != Phase::Src {
                        Some(format!(
                            "cannot be modified in phase {}: active src changelists \
                             are only editable in IMPL phase.",
                            phase.label(),
                        ))
                    } else {
                        None // allowed
                    }
                },
            }
        },
    }
}

/// Get modified files in design_rounds/ (staged + unstaged), excluding
/// pure additions. A pure addition is the legitimate introduction of a
/// new changelist file: the file did not exist before the working
/// change, so there is nothing to protect against modification. The
/// immutability lint exists to block edits to ALREADY-LOCKED CLs, not
/// to block their initial creation.
///
/// Uses `git diff --name-status` to distinguish:
/// - `A` (added): skip; first appearance of the file
/// - `M` (modified), `R*` (renamed), `D` (deleted): include; an edit
///   to an existing CL file
fn get_modified_in_design_rounds(workspace_root: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();

    // Staged changes
    if let Some(output) = run_git(workspace_root, &[
        "diff",
        "--cached",
        "--name-status",
        "--relative",
        "--",
        "design_rounds/",
    ]) {
        collect_non_additions(&output, "staged", &mut files);
    }

    // Unstaged tracked changes
    if let Some(output) = run_git(workspace_root, &[
        "diff",
        "--name-status",
        "--relative",
        "--",
        "design_rounds/",
    ]) {
        collect_non_additions(&output, "unstaged", &mut files);
    }

    files
}

/// The lifecycle status a changelist filename carries, paired with the
/// stem the status suffix sits on.
///
/// `foo.doc.md` is `("foo.doc", Active)`, `foo.doc.lock.md` is
/// `("foo.doc", Locked)`, `foo.doc.deprecated.md` is
/// `("foo.doc", Deprecated)`.
fn split_status(name: &str) -> Option<(&str, ClStatus)> {
    let stem = name.strip_suffix(".md")?;
    if let Some(base) = stem.strip_suffix(".lock") {
        Some((base, ClStatus::Locked))
    } else if let Some(base) = stem.strip_suffix(".deprecated") {
        Some((base, ClStatus::Deprecated))
    } else {
        Some((stem, ClStatus::Active))
    }
}

/// Whether renaming `old` to `new` is one of the lifecycle transitions the
/// mockspace subcommands perform.
///
/// The subcommands move a changelist between statuses by renaming it, leaving
/// the content untouched. Four transitions are reachable:
///
/// - `lock` renames active to locked.
/// - `deprecate` renames active to deprecated.
/// - `unlock` renames locked back to active.
/// - `unlock` followed by `deprecate` composes, in one staged diff, to locked
///   renamed straight to deprecated.
///
/// Nothing leads out of deprecated: a deprecated changelist is frozen forever,
/// so a rename that would resurrect one is a violation and not a transition.
/// The stem must be identical either way, since a transition never renames the
/// changelist itself.
fn is_status_suffix_rename(old: &str, new: &str) -> bool {
    let (Some((old_stem, old_status)), Some((new_stem, new_status))) =
        (split_status(old), split_status(new))
    else {
        return false;
    };
    if old_stem != new_stem {
        return false;
    }
    matches!(
        (old_status, new_status),
        (ClStatus::Active, ClStatus::Locked)
            | (ClStatus::Active, ClStatus::Deprecated)
            | (ClStatus::Locked, ClStatus::Active)
            | (ClStatus::Locked, ClStatus::Deprecated)
    )
}

/// Parse `git diff --name-status` output and append non-A entries.
/// Each input line is `<STATUS>\t<PATH>` (or `<R100>\t<OLD>\t<NEW>`
/// for renames). We treat `A` (and the unlikely `A100`) as pure
/// additions to skip. Renames are kept (the post-rename path).
fn collect_non_additions(output: &str, source: &str, files: &mut Vec<(String, String)>) {
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        let status = match parts.next() {
            Some(s) => s,
            None => continue,
        };
        if status.is_empty() {
            continue;
        }
        if status.starts_with('A') {
            // Pure addition: first appearance, nothing to protect.
            continue;
        }
        // For modifications, take the first path field. For renames
        // (`R100\t<old>\t<new>`), git lists both; take the new path.
        let path = if status.starts_with('R') || status.starts_with('C') {
            let old = match parts.next() {
                Some(p) => p,
                None => continue,
            };
            let new = match parts.next() {
                Some(p) => p,
                None => continue,
            };
            // The lock and deprecate transitions ARE renames: `cargo mock lock`
            // moves `<cl>.md` to `<cl>.lock.md` with the content untouched. So
            // running the command produced a state this lint rejected, which
            // made the documented flow unusable. A status-suffix rename at full
            // similarity is that transition and is allowed; any other rename of
            // a frozen changelist is still a modification.
            if status == "R100" && is_status_suffix_rename(old, new) {
                continue;
            }
            new
        } else {
            match parts.next() {
                Some(p) => p,
                None => continue,
            }
        };
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            add_unique(files, trimmed, source);
        }
    }
}

fn add_unique(list: &mut Vec<(String, String)>, file: &str, source: &str) {
    if !list.iter().any(|(f, _)| f == file) {
        list.push((file.to_string(), source.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::collect_non_additions;

    #[test]
    fn pure_addition_skipped() {
        let mut out = Vec::new();
        collect_non_additions(
            "A\tdesign_rounds/202605241200_changelist.doc.lock.md\n",
            "staged",
            &mut out,
        );
        assert!(
            out.is_empty(),
            "added file should not appear in modified-list"
        );
    }

    #[test]
    fn modification_included() {
        let mut out = Vec::new();
        collect_non_additions("M\tdesign_rounds/foo.doc.lock.md\n", "staged", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/foo.doc.lock.md");
    }

    #[test]
    fn rename_uses_new_path() {
        // a rename that is NOT the lock/deprecate status-suffix transition is a
        // real modification of a frozen changelist, collected under its new path.
        let mut out = Vec::new();
        collect_non_additions(
            "R100\tdesign_rounds/foo.doc.md\tdesign_rounds/bar.doc.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/bar.doc.md");
    }

    /// Every ordered pair of the three statuses, so no direction is left
    /// unnamed. A transition the subcommands perform is allowed; every other
    /// pair is a modification of a frozen changelist and is collected.
    ///
    /// Reading the table: the four allowed rows are `lock`, `deprecate`,
    /// `unlock`, and `unlock` composed with `deprecate` in one staged diff.
    /// The three rows out of deprecated are the ones that matter, because a
    /// deprecated changelist is frozen forever and resurrecting it must not
    /// pass as a lifecycle move.
    #[test]
    fn every_status_transition_is_classified() {
        const SUFFIXES: [&str; 3] = ["", ".lock", ".deprecated"];
        // (old_index, new_index) -> allowed
        const ALLOWED: [[bool; 3]; 3] = [
            //  ->active  ->lock  ->deprecated
            [false, true, true],   // from active
            [true, false, true],   // from locked
            [false, false, false], // from deprecated
        ];

        for (oi, old_suffix) in SUFFIXES.iter().enumerate() {
            for (ni, new_suffix) in SUFFIXES.iter().enumerate() {
                let old = format!("design_rounds/foo.doc{old_suffix}.md");
                let new = format!("design_rounds/foo.doc{new_suffix}.md");
                let mut out = Vec::new();
                collect_non_additions(
                    &format!("R100\t{old}\t{new}\n"),
                    "staged",
                    &mut out,
                );
                if ALLOWED[oi][ni] {
                    assert!(
                        out.is_empty(),
                        "{old} -> {new} is a lifecycle transition and must not be \
                         reported as a modification",
                    );
                } else {
                    assert_eq!(
                        out.len(),
                        1,
                        "{old} -> {new} is not a lifecycle transition and must be \
                         reported",
                    );
                }
            }
        }
    }

    /// A stem rename carrying a status suffix change at the same time is still
    /// a rename of the changelist, not a transition of it.
    #[test]
    fn stem_rename_with_status_change_is_reported() {
        let mut out = Vec::new();
        collect_non_additions(
            "R100\tdesign_rounds/foo.doc.md\tdesign_rounds/bar.doc.lock.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/bar.doc.lock.md");
    }

    /// Below full similarity the content changed, so it is an edit wearing a
    /// transition's filename.
    #[test]
    fn partial_similarity_rename_is_reported() {
        let mut out = Vec::new();
        collect_non_additions(
            "R087\tdesign_rounds/foo.doc.md\tdesign_rounds/foo.doc.lock.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/foo.doc.lock.md");
    }

    #[test]
    fn deletion_included() {
        let mut out = Vec::new();
        collect_non_additions("D\tdesign_rounds/foo.doc.md\n", "staged", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/foo.doc.md");
    }

    #[test]
    fn mixed_input_filters_only_additions() {
        let mut out = Vec::new();
        collect_non_additions(
            "A\tdesign_rounds/new.doc.lock.md\nM\tdesign_rounds/existing.doc.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/existing.doc.md");
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}
