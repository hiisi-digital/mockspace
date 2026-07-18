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
use crate::{CrossCrateLint, LintContext, LintError};

const LINT_NAME: &str = "changelist-immutability";

pub struct ChangelistImmutability;

impl CrossCrateLint for ChangelistImmutability {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn source_only(&self) -> bool { false }

    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        let workspace_root = match crates.first() {
            Some((_, ctx)) => ctx.workspace_root,
            None => return Vec::new(),
        };

        let design_rounds = workspace_root.join("design_rounds");
        let all_cls = changelist_helpers::find_changelists(&design_rounds);
        if all_cls.is_empty() {
            return Vec::new();
        }

        let phase = changelist_helpers::current_phase(&design_rounds);

        // Get all modified files in design_rounds/.
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
                    format!(
                        "changelist `{}` ({source}) {msg}",
                        cl.filename,
                    ),
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
                "cannot be modified — it is locked and frozen forever. \
                 Use SHAME.md.tmpl to document gaps discovered during execution."
            ))
        }
        ClStatus::Deprecated => {
            Some(format!(
                "cannot be modified — it is deprecated and frozen forever."
            ))
        }
        ClStatus::Active => {
            match cl.kind {
                ClKind::Doc => {
                    if phase != Phase::Doc {
                        Some(format!(
                            "cannot be modified in phase {} — active doc changelists \
                             are only editable in DOC phase.",
                            phase.label(),
                        ))
                    } else {
                        None // allowed
                    }
                }
                ClKind::Src => {
                    if phase != Phase::Src {
                        Some(format!(
                            "cannot be modified in phase {} — active src changelists \
                             are only editable in IMPL phase.",
                            phase.label(),
                        ))
                    } else {
                        None // allowed
                    }
                }
            }
        }
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
        "diff", "--cached", "--name-status", "--relative", "--", "design_rounds/",
    ]) {
        collect_non_additions(&output, "staged", &mut files);
    }

    // Unstaged tracked changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--name-status", "--relative", "--", "design_rounds/",
    ]) {
        collect_non_additions(&output, "unstaged", &mut files);
    }

    files
}

/// Whether `new` is `old` with a lifecycle status suffix added.
///
/// `202607180723_changelist.doc.md` to `202607180723_changelist.doc.lock.md`
/// is the lock transition; the same shape with `.deprecated` is the deprecate
/// transition. Both rename without touching content.
fn is_status_suffix_rename(old: &str, new: &str) -> bool {
    let Some(stem) = old.strip_suffix(".md") else {
        return false;
    };
    new == format!("{stem}.lock.md") || new == format!("{stem}.deprecated.md")
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
        assert!(out.is_empty(), "added file should not appear in modified-list");
    }

    #[test]
    fn modification_included() {
        let mut out = Vec::new();
        collect_non_additions(
            "M\tdesign_rounds/foo.doc.lock.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/foo.doc.lock.md");
    }

    #[test]
    fn rename_uses_new_path() {
        let mut out = Vec::new();
        collect_non_additions(
            "R100\tdesign_rounds/foo.doc.md\tdesign_rounds/foo.doc.lock.md\n",
            "staged",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "design_rounds/foo.doc.lock.md");
    }

    #[test]
    fn deletion_included() {
        let mut out = Vec::new();
        collect_non_additions(
            "D\tdesign_rounds/foo.doc.md\n",
            "staged",
            &mut out,
        );
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
