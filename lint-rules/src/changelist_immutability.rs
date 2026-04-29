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

/// Get modified files in design_rounds/ (staged + unstaged).
fn get_modified_in_design_rounds(workspace_root: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();

    // Staged changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--cached", "--name-only", "--relative", "--", "design_rounds/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if !file.is_empty() {
                add_unique(&mut files, file, "staged");
            }
        }
    }

    // Unstaged tracked changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--name-only", "--relative", "--", "design_rounds/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if !file.is_empty() {
                add_unique(&mut files, file, "unstaged");
            }
        }
    }

    files
}

fn add_unique(list: &mut Vec<(String, String)>, file: &str, source: &str) {
    if !list.iter().any(|(f, _)| f == file) {
        list.push((file.to_string(), source.to_string()));
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
