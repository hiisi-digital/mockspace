//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Cross-crate lint: block crate doc edits after doc CL is locked,
//! and block source edits after src CL is locked.
//!
//! Phase enforcement:
//! - Doc templates blocked if doc CL is locked (DRAFT, IMPL, CLOSED).
//! - Source files blocked if src CL is locked (CLOSED).
//!
//! SHAME.md.tmpl is always exempt: it is the escape valve for
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

use crate::changelist_helpers::{self, Phase};
use crate::src_layout::{self, SrcLayout};
use crate::{Lint, RepoLint, LintError, RepoContext};

const LINT_NAME: &str = "changelist-lock";

pub struct ChangelistLock;

impl Lint for ChangelistLock {
    fn name(&self) -> &'static str {
        LINT_NAME
    }
    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistLock {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        // The mock dir comes straight from the context. Previously this stole a
        // `workspace_root` from `crates.first()` and returned no findings when
        // there were no crates, which silently disabled the gate in a repo whose
        // taxonomy had not been settled yet.
        let workspace_root = ctx.mock_dir;
        let layout = SrcLayout::new(workspace_root, ctx.src_dirs);

        let design_rounds = workspace_root.join("design_rounds");
        let phase = changelist_helpers::current_phase(&design_rounds);

        let doc_locked = matches!(phase, Phase::SrcPlan | Phase::Src | Phase::Done);
        let src_locked = matches!(phase, Phase::Done);

        // If nothing is locked, this lint has nothing to enforce.
        if !doc_locked && !src_locked {
            return Vec::new();
        }

        // Identify the locked CL names for error messages.
        let locked_doc_name =
            changelist_helpers::find_locked_doc_cl(&design_rounds).map(|cl| cl.filename);
        let locked_src_name =
            changelist_helpers::find_locked_src_cl(&design_rounds).map(|cl| cl.filename);

        let mut errors = Vec::new();

        // Check doc templates if doc CL is locked.
        if doc_locked {
            let doc_files = get_modified_crate_files(workspace_root, &layout, true);
            let cl_name = locked_doc_name.as_deref().unwrap_or("doc changelist");

            for (file, source) in doc_files {
                let crate_name = layout.package_name(&file).unwrap_or_else(|| "unknown".to_string());
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
            let src_files = crate::fmt_only::drop_fmt_only(
                workspace_root,
                get_modified_crate_files(workspace_root, &layout, false),
            );
            let cl_name = locked_src_name.as_deref().unwrap_or("src changelist");

            for (file, source) in src_files {
                let crate_name = layout.package_name(&file).unwrap_or_else(|| "unknown".to_string());
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

/// Modified files under the project's source directories. `docs` picks the doc
/// templates a lock freezes; otherwise the source a lock closes.
fn get_modified_crate_files(
    workspace_root: &Path,
    layout: &SrcLayout,
    docs: bool,
) -> Vec<(String, String)> {
    src_layout::changed_files(workspace_root, layout, |f| {
        if docs { is_locked_doc(layout, f) } else { is_source(layout, f) }
    })
}

/// A doc a lock freezes: a `.md` or `.md.tmpl` under a source directory, with
/// `SHAME.md.tmpl` carved out because that file is where gaps found during
/// execution are written down, which is exactly when the lock is on.
fn is_locked_doc(layout: &SrcLayout, file: &str) -> bool {
    if !layout.holds(file) {
        return false;
    }
    let is_doc = file.ends_with(".md.tmpl") || file.ends_with(".md");
    is_doc && !crate::is_shame_template(file)
}

// FIXME: `.rs` is rust's convention standing in for the project's. `src_dirs`
// says where source is and nothing says what counts as source there, so a
// project writing zig or typescript gets a lock that guards none of it while
// reporting cleanly. Same gap as `changelist_required.rs`, same fix: an
// extension set per source directory, or a language key carrying one.
fn is_source(layout: &SrcLayout, file: &str) -> bool {
    layout.holds(file) && file.ends_with(".rs")
}

#[cfg(test)]
mod is_locked_doc_tests {
    use std::path::{Path, PathBuf};

    use super::{is_locked_doc, is_source};
    use crate::src_layout::SrcLayout;

    fn default_layout() -> SrcLayout {
        SrcLayout::new(Path::new("/m"), &[PathBuf::from("/m/crates")])
    }

    fn renamed_layout() -> SrcLayout {
        SrcLayout::new(Path::new("/m"), &[PathBuf::from("/m/libs")])
    }

    #[test]
    fn crate_docs_are_locked_and_only_shame_itself_is_exempt() {
        let l = default_layout();
        assert!(is_locked_doc(&l, "crates/foo/DESIGN.md.tmpl"));
        assert!(is_locked_doc(&l, "crates/foo/README.md"));
        assert!(!is_locked_doc(&l, "crates/foo/SHAME.md.tmpl"));
        assert!(!is_locked_doc(&l, "crates/SHAME.md.tmpl"));
        // A suffix match would exempt these, opening the lock gate for
        // templates nobody carved out.
        assert!(is_locked_doc(&l, "crates/foo/NOT_SHAME.md.tmpl"));
        assert!(is_locked_doc(&l, "crates/foo/DESIGN_SHAME.md.tmpl"));
    }

    #[test]
    fn files_outside_crates_and_non_docs_are_not_locked_docs() {
        let l = default_layout();
        assert!(!is_locked_doc(&l, ""));
        assert!(!is_locked_doc(&l, "design_rounds/foo.md"));
        assert!(!is_locked_doc(&l, "crates/foo/src/lib.rs"));
    }

    /// The reason the layout is threaded through at all: a project that moved
    /// its packages is locked where they are, and not where they used to be.
    #[test]
    fn a_renamed_source_directory_is_where_the_lock_applies() {
        let l = renamed_layout();
        assert!(is_locked_doc(&l, "libs/foo/DESIGN.md.tmpl"));
        assert!(is_source(&l, "libs/foo/src/lib.rs"));
        assert!(!is_locked_doc(&l, "crates/foo/DESIGN.md.tmpl"));
        assert!(!is_source(&l, "crates/foo/src/lib.rs"));
    }
}
