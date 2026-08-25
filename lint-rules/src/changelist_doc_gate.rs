//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Repo lint: doc template edits require the DOC phase.
//!
//! A doc template under one of the project's declared source directories may
//! only change during `Phase::Doc`, which is when an unlocked doc CL exists.
//! Which directories those are comes from `src_dirs` rather than from the word
//! `crates`; `src_layout` holds that.
//!
//! Blocked in: TOPIC (no CL), DRAFT (doc CL locked), IMPL (source
//! window), CLOSED (round complete).
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged doc template changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use crate::changelist_helpers::{self, Phase};
use crate::src_layout::{self, SrcLayout};
use crate::{Lint, LintError, RepoContext, RepoLint};

const LINT_NAME: &str = "changelist-doc-gate";

pub struct ChangelistDocGate;

impl Lint for ChangelistDocGate {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistDocGate {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        // The mock dir comes straight from the context. Previously this stole a
        // `workspace_root` from `crates.first()` and returned no findings when
        // there were no crates, which silently disabled the gate in a repo whose
        // taxonomy had not been settled yet.
        let workspace_root = ctx.mock_dir;
        let layout = SrcLayout::new(workspace_root, ctx.src_dirs);

        let design_rounds = workspace_root.join("design_rounds");
        let phase = changelist_helpers::current_phase(&design_rounds);

        // Doc templates are allowed only in Phase::Doc.
        if phase == Phase::Doc {
            return Vec::new();
        }

        let violating_files =
            src_layout::changed_files(workspace_root, &layout, |f| is_doc_template(&layout, f));

        violating_files
            .into_iter()
            .map(|(file, source)| {
                let crate_name = layout
                    .package_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());

                let phase_hint = match phase {
                    Phase::Topic => {
                        "phase TOPIC: only topic files allowed. \
                         Create an unlocked doc changelist to open the docs window"
                    },
                    Phase::SrcPlan => {
                        // The phase below is the user-visible label; the variant
                        // name (SrcPlan) preserves the file-suffix machinery.
                        "phase DRAFT: doc CL is locked. \
                         Doc edits are frozen after locking. \
                         Use SHAME.md.tmpl for gaps discovered during execution"
                    },
                    Phase::Src => {
                        "phase IMPL: source window open, doc edits blocked. \
                         Doc edits are frozen after locking. \
                         Use SHAME.md.tmpl for gaps discovered during execution"
                    },
                    Phase::Done => {
                        "phase CLOSED: round complete. \
                         Start a new design round to make further doc changes"
                    },
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

/// A doc template is a `.md.tmpl` or `.md` file under a source directory, with
/// `SHAME.md.tmpl` carved out. SHAME is the escape valve for gaps discovered
/// during execution, and this gate's own error messages send people there, so
/// it has to be writable in every phase.
fn is_doc_template(layout: &SrcLayout, file: &str) -> bool {
    if !layout.holds(file) || crate::is_shame_template(file) {
        return false;
    }
    file.ends_with(".md.tmpl") || file.ends_with(".md")
}

#[cfg(test)]
mod is_doc_template_tests {
    use std::path::{Path, PathBuf};

    use crate::src_layout::SrcLayout;

    fn is_doc_template(file: &str) -> bool {
        let l = SrcLayout::new(Path::new("/m"), &[PathBuf::from("/m/crates")]);
        super::is_doc_template(&l, file)
    }

    /// The exemption is for the file named `SHAME.md.tmpl`, so it matches
    /// a whole path component. A suffix match also exempts any template
    /// whose name merely ends in those characters, which silently opens
    /// the doc gate for files nobody exempted.
    #[test]
    fn only_the_shame_template_itself_is_exempt() {
        assert!(!is_doc_template("crates/foo/SHAME.md.tmpl"));
        assert!(!is_doc_template("crates/foo-bar/SHAME.md.tmpl"));
        assert!(!is_doc_template("crates/SHAME.md.tmpl"));
        assert!(is_doc_template("crates/foo/NOT_SHAME.md.tmpl"));
        assert!(is_doc_template("crates/foo/DESIGN_SHAME.md.tmpl"));
    }

    #[test]
    fn design_md_tmpl_is_gated() {
        assert!(is_doc_template("crates/foo/DESIGN.md.tmpl"));
    }

    #[test]
    fn plain_md_inside_crates_is_gated() {
        assert!(is_doc_template("crates/foo/notes.md"));
    }

    #[test]
    fn files_outside_crates_are_not_gated() {
        assert!(!is_doc_template("design_rounds/x.md"));
        assert!(!is_doc_template("README.md"));
    }

    /// The reason the layout is threaded through: a project that renamed its
    /// source directory is gated there, and its old name is now an ordinary
    /// directory the gate has no business in.
    #[test]
    fn a_renamed_source_directory_is_where_the_gate_applies() {
        let l = SrcLayout::new(Path::new("/m"), &[PathBuf::from("/m/libs")]);
        assert!(super::is_doc_template(&l, "libs/foo/DESIGN.md.tmpl"));
        assert!(!super::is_doc_template(&l, "crates/foo/DESIGN.md.tmpl"));
    }
}
