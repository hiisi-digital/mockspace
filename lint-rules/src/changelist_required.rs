//! Cross-crate lint: source code changes require the IMPL phase.
//!
//! Source changes (`*.rs` in `crates/`) are only allowed during
//! `Phase::Src` (label IMPL) — when a doc CL is locked AND an unlocked
//! src CL exists.
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged source changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, Phase};
use crate::{CrossCrateLint, LintContext, LintError};

const LINT_NAME: &str = "changelist-required";

pub struct ChangelistRequired;

impl CrossCrateLint for ChangelistRequired {
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

        // Source changes are allowed only in Phase::Src.
        if phase == Phase::Src {
            return Vec::new();
        }

        // Check if any .rs source files are modified.
        let modified = get_all_modified_rs_files(workspace_root);

        modified
            .into_iter()
            .filter(|(file, _)| {
                // Skip nuked crates — intentionally wiped source is not a
                // phase violation. Check the crate's lib.rs for the nuke marker.
                let crate_name = extract_crate_name(file).unwrap_or_default();
                let librs = workspace_root.join("crates").join(&crate_name).join("src/lib.rs");
                !std::fs::read_to_string(&librs)
                    .map(|s| s.contains("Nuked by"))
                    .unwrap_or(false)
            })
            .map(|(file, source)| {
                let crate_name = extract_crate_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());

                let phase_hint = match phase {
                    Phase::Topic => {
                        "phase TOPIC: only topic files allowed. \
                         Create a doc changelist, lock it, then create a src changelist \
                         to open the source window"
                    }
                    Phase::Doc => {
                        "phase DOC: docs window open, source blocked. \
                         Lock the doc changelist and create a src changelist \
                         to open the source window"
                    }
                    Phase::SrcPlan => {
                        "phase DRAFT: doc CL locked, but no src changelist yet. \
                         Create an unlocked src changelist to open the source window"
                    }
                    Phase::Done => {
                        "phase CLOSED: round complete, both changelists locked. \
                         Start a new design round to make further changes"
                    }
                    Phase::Src => unreachable!(),
                };

                LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "source file `{file}` ({source}) cannot be modified — \
                         {phase_hint}. Revert this change before committing.",
                    ),
                )
            })
            .collect()
    }
}

/// Get all modified .rs files in crates/ (staged + unstaged + untracked).
fn get_all_modified_rs_files(workspace_root: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();

    // Staged changes
    if let Some(output) = run_git(workspace_root, &[
        "diff", "--cached", "--name-only", "--relative", "--", "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if is_crate_source(file) {
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
            if is_crate_source(file) {
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
            if is_crate_source(file) {
                add_unique(&mut files, file, "untracked");
            }
        }
    }

    files
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

/// Extract crate name from a path like `crates/<crate-name>/src/lib.rs`.
fn extract_crate_name(path: &str) -> Option<String> {
    let after_crates = path.strip_prefix("crates/")?;
    let end = after_crates.find('/')?;
    Some(after_crates[..end].to_string())
}
