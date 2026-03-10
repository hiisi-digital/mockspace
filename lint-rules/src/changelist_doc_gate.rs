//! Cross-crate lint: doc template edits require the DOC phase.
//!
//! Doc template changes in `crates/` are only allowed during
//! `Phase::Doc` — when an unlocked doc CL exists.
//!
//! Blocked in: TOPIC (no CL), SRC-PLAN (doc CL locked), SRC (source
//! window), DONE (round complete).
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged doc template changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, Phase};
use crate::{CrossCrateLint, LintContext, LintError};

const LINT_NAME: &str = "changelist-doc-gate";

pub struct ChangelistDocGate;

impl CrossCrateLint for ChangelistDocGate {
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

        // Doc templates are allowed only in Phase::Doc.
        if phase == Phase::Doc {
            return Vec::new();
        }

        // Scan for doc template changes.
        let mut violating_files: Vec<(String, String)> = Vec::new();

        // 1. Check staged changes.
        if let Some(output) = run_git(workspace_root, &[
            "diff", "--cached", "--name-only", "--relative", "--", "crates/",
        ]) {
            for line in output.lines() {
                let file = line.trim();
                if is_doc_template(file) {
                    add_unique(&mut violating_files, file, "staged");
                }
            }
        }

        // 2. Check unstaged working tree changes.
        if let Some(output) = run_git(workspace_root, &[
            "diff", "--name-only", "--relative", "--", "crates/",
        ]) {
            for line in output.lines() {
                let file = line.trim();
                if is_doc_template(file) {
                    add_unique(&mut violating_files, file, "unstaged");
                }
            }
        }

        // 3. Check untracked files.
        if let Some(output) = run_git(workspace_root, &[
            "ls-files", "--others", "--exclude-standard", "--", "crates/",
        ]) {
            for line in output.lines() {
                let file = line.trim();
                if is_doc_template(file) {
                    add_unique(&mut violating_files, file, "untracked");
                }
            }
        }

        violating_files
            .into_iter()
            .map(|(file, source)| {
                let crate_name = extract_crate_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());

                let phase_hint = match phase {
                    Phase::Topic => {
                        "phase TOPIC: only topic files allowed. \
                         Create an unlocked doc changelist to open the docs window"
                    }
                    Phase::SrcPlan => {
                        "phase SRC-PLAN: doc CL is locked. \
                         Doc edits are frozen after locking. \
                         Use SHAME.md.tmpl for gaps discovered during execution"
                    }
                    Phase::Src => {
                        "phase SRC: source window open, doc edits blocked. \
                         Doc edits are frozen after locking. \
                         Use SHAME.md.tmpl for gaps discovered during execution"
                    }
                    Phase::Done => {
                        "phase DONE: round complete. \
                         Start a new design round to make further doc changes"
                    }
                    Phase::Doc => unreachable!(),
                };

                LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "doc template `{file}` ({source}) changed outside DOC phase \
                         ({phase_hint}). Revert this change before committing.",
                    ),
                )
            })
            .collect()
    }
}

/// A doc template is a `.md.tmpl` or `.md` file inside `crates/`.
fn is_doc_template(file: &str) -> bool {
    if file.is_empty() || !file.starts_with("crates/") {
        return false;
    }
    file.ends_with(".md.tmpl") || file.ends_with(".md")
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
