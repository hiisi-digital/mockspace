//! Cross-crate lint: block crate doc edits after doc CL is locked,
//! and block source edits after src CL is locked.
//!
//! Phase enforcement:
//! - Doc templates blocked if doc CL is locked (DRAFT, IMPL, CLOSED).
//! - Source files blocked if src CL is locked (CLOSED).
//!
//! SHAME.md.tmpl is always exempt — it is the escape valve for
//! documenting types discovered during changelist execution.
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged changes will block the commit. Revert disallowed changes
//! before committing.
//!
//! Agent files, root templates, and other non-crate docs can change freely.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, Phase};
use crate::{CrossCrateLint, LintContext, LintError};

const LINT_NAME: &str = "changelist-lock";

pub struct ChangelistLock;

impl CrossCrateLint for ChangelistLock {
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
        let phase = changelist_helpers::current_phase(&design_rounds);

        let doc_locked = matches!(phase, Phase::SrcPlan | Phase::Src | Phase::Done);
        let src_locked = matches!(phase, Phase::Done);

        // If nothing is locked, this lint has nothing to enforce.
        if !doc_locked && !src_locked {
            return Vec::new();
        }

        // Identify the locked CL names for error messages.
        let locked_doc_name = changelist_helpers::find_locked_doc_cl(&design_rounds)
            .map(|cl| cl.filename);
        let locked_src_name = changelist_helpers::find_locked_src_cl(&design_rounds)
            .map(|cl| cl.filename);

        let mut errors = Vec::new();

        // Check doc templates if doc CL is locked.
        if doc_locked {
            let doc_files = get_modified_crate_files(workspace_root, true);
            let cl_name = locked_doc_name.as_deref().unwrap_or("doc changelist");

            for (file, source) in doc_files {
                let crate_name = extract_crate_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());
                errors.push(LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "crate doc `{file}` ({source}) changed while doc changelist \
                         `{cl_name}` is locked (phase {phase}). \
                         Doc template edits are only allowed in DOC phase. \
                         Revert this change or use SHAME.md.tmpl for gaps \
                         discovered during execution.",
                        phase = phase.label(),
                    ),
                ));
            }
        }

        // Check source files if src CL is locked.
        if src_locked {
            let src_files = get_modified_crate_files(workspace_root, false);
            let cl_name = locked_src_name.as_deref().unwrap_or("src changelist");

            for (file, source) in src_files {
                let crate_name = extract_crate_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());
                errors.push(LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "source file `{file}` ({source}) changed while src changelist \
                         `{cl_name}` is locked (phase CLOSED). \
                         Round is complete. Start a new design round to make \
                         further changes.",
                    ),
                ));
            }
        }

        errors
    }
}

/// Get modified files in crates/. If `docs` is true, return doc templates
/// (excluding SHAME.md.tmpl). If false, return .rs source files.
fn get_modified_crate_files(workspace_root: &Path, docs: bool) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let filter = if docs { is_locked_doc } else { is_crate_source };

    // Staged changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--cached", "--name-only", "--relative", "--", "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if filter(file) {
                add_unique(&mut files, file, "staged");
            }
        }
    }

    // Unstaged tracked changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--name-only", "--relative", "--", "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if filter(file) {
                add_unique(&mut files, file, "unstaged");
            }
        }
    }

    // Untracked files
    if let Some(output) = run_git(workspace_root, &[
        "ls-files", "--others", "--exclude-standard", "--", "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if filter(file) {
                add_unique(&mut files, file, "untracked");
            }
        }
    }

    files
}

/// A file is a locked doc if it's a `.md` or `.md.tmpl` inside `crates/`,
/// excluding `SHAME.md.tmpl`.
fn is_locked_doc(file: &str) -> bool {
    if file.is_empty() || !file.starts_with("crates/") {
        return false;
    }
    let is_doc = file.ends_with(".md.tmpl") || file.ends_with(".md");
    let is_shame = file.ends_with("SHAME.md.tmpl");
    is_doc && !is_shame
}

fn is_crate_source(file: &str) -> bool {
    !file.is_empty() && file.starts_with("crates/") && file.ends_with(".rs")
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

/// Extract crate name from a path like `crates/<crate-name>/DESIGN.md.tmpl`.
fn extract_crate_name(path: &str) -> Option<String> {
    let after_crates = path.strip_prefix("crates/")?;
    let end = after_crates.find('/')?;
    Some(after_crates[..end].to_string())
}
