//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Repo lint: source code changes require the IMPL phase.
//!
//! A `*.rs` file under one of the project's declared source directories may
//! only change during `Phase::Src` (label IMPL), which is when a doc CL is
//! locked and an unlocked src CL exists.
//!
//! Which directories those are comes from `src_dirs` rather than from the word
//! `crates`, so a project that renamed or grouped its packages is guarded where
//! its source actually is. `src_layout` holds that.
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged source changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::{Path, PathBuf};

use crate::changelist_helpers::{self, Phase};
use crate::src_layout::{self, SrcLayout};
use crate::{Lint, LintError, RepoContext, RepoLint, Severity, View};

#[path = "changelist_required_scaffold.rs"]
mod scaffold;
#[cfg(test)]
use scaffold::declares_nothing;
use scaffold::passes_outside_impl;
pub use scaffold::{NUKE_MARKER, nuke_stub};

const LINT_NAME: &str = "changelist-required";

pub struct ChangelistRequired;

impl Lint for ChangelistRequired {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    /// The design-round gate. Blocking is the tool.
    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistRequired {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        // The mock dir comes straight from the context. Previously this stole a
        // `workspace_root` from `crates.first()` and returned no findings when
        // there were no crates, which silently disabled the gate in a repo whose
        // taxonomy had not been settled yet.
        let workspace_root = ctx.mock_dir;
        let layout = SrcLayout::new(workspace_root, ctx.src_dirs());

        let design_rounds = workspace_root.join("design_rounds");
        let phase = changelist_helpers::current_phase(ctx.view(), &design_rounds);

        // Source changes are allowed only in Phase::Src.
        if phase == Phase::Src {
            return Vec::new();
        }

        // Check if any .rs source files are modified.
        let modified = crate::fmt_only::drop_fmt_only(
            ctx.view(),
            workspace_root,
            // FIXME: `.rs` is the half of this that is still rust's convention
            // rather than the project's. The directories now come from
            // `src_dirs`; what counts as source in them does not, and nothing
            // in `mockspace.toml` names it. A project writing zig or typescript
            // under a declared source directory gets a gate that matches
            // nothing, passes, and looks exactly like a gate that works, which
            // is verbatim the failure `src_layout` was written to end. Wants an
            // extension set per source directory, or a language key carrying
            // one. Catalogued at `a_source_gate_guards_the_extensions_the_project_writes`.
            src_layout::changed_files(ctx.view(), workspace_root, &layout, ctx.ref_files(), |f| {
                layout.holds(f) && f.ends_with(".rs")
            }),
        );

        modified
            .into_iter()
            .filter(|(file, source)| {
                let view = ctx.view();
                // Read the STAGED blob, not the working tree. What gets
                // committed is the staged content, so judging the worktree
                // would let someone stage real source, overwrite the worktree
                // copy with comments, and commit past the gate.
                let content = staged_or_worktree(view, workspace_root, file, source);
                let passes = passes_outside_impl(view, workspace_root, &layout, file, content);
                // A staged file edited again in the working tree is listed
                // once, as staged. `git commit -a`, the push gate and a build
                // carry the working-tree text, so outside IMPL it is judged too.
                let worktree = (source == "staged")
                    .then(|| view.text(workspace_root.join(file)))
                    .flatten()
                    .filter(|w| Some(*w) != content);
                !(passes
                    && worktree.is_none_or(|w| {
                        passes_outside_impl(view, workspace_root, &layout, file, Some(w))
                    }))
            })
            .map(|(file, source)| {
                let crate_name = layout
                    .package_name(&file)
                    .unwrap_or_else(|| "unknown".to_string());

                let phase_hint = match phase {
                    Phase::Topic => {
                        "phase TOPIC: only topic files allowed. \
                         Create a doc changelist, lock it, then create a src changelist \
                         to open the source window"
                    },
                    Phase::Doc => {
                        "phase DOC: docs window open, source blocked. \
                         Lock the doc changelist and create a src changelist \
                         to open the source window"
                    },
                    Phase::SrcPlan => {
                        "phase DRAFT: doc CL locked, but no src changelist yet. \
                         Create an unlocked src changelist to open the source window"
                    },
                    Phase::Done => {
                        "phase CLOSED: round complete, both changelists locked. \
                         Start a new design round to make further changes"
                    },
                    Phase::Src => unreachable!(),
                };

                LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "source file `{file}` ({source}) cannot be modified: \
                         {phase_hint}. Revert this change before committing.",
                    ),
                )
            })
            .collect()
    }
}

/// The gate's verdict on one source file as an edit not yet made would leave
/// it: `true` when committing `text` at `file` passes, whatever the phase.
///
/// `file` is relative to `mock_dir`, as the gate reads paths. This is the
/// question an agent hook asks before a write lands, so the hook and the commit
/// give the same text the same verdict. The fmt-only arm is not asked here: a
/// reformatting the hook refuses is the formatter's to make, after the edit.
///
/// The repository is read through `view`, which the caller takes of a reading
/// of its own, so the hook asks the same questions the lint asks.
pub fn admits_edit(view: &View<'_>, mock_dir: &Path, src_dirs: &[PathBuf], file: &str, text: &str) -> bool {
    let layout = SrcLayout::new(mock_dir, src_dirs);
    if !(layout.holds(file) && file.ends_with(".rs")) {
        return true;
    }
    changelist_helpers::current_phase(view, &mock_dir.join("design_rounds")) == Phase::Src
        || passes_outside_impl(view, mock_dir, &layout, file, Some(text))
}

/// The content that would actually be committed.
///
/// For a staged entry that is the blob in the index, which is what a plain
/// `git commit` writes. Reading the working tree instead would judge content
/// the commit does not contain, and the difference is exactly where a gate
/// gets walked past.
///
/// Known gap, tracked #41: under `git commit -a` a file staged earlier and
/// then further modified commits the *worktree* blob, so this reads content
/// the commit will not carry. Detection is unaffected (the file is flagged
/// either way); only content-based judgment can go stale, and the case is
/// catalogued as an ignored test below. `changelist-required` closes it for
/// itself by judging a staged file's working-tree text as well; the fmt-only
/// exemption still reads the index alone. Closing it here needs a signal for
/// which commit shape is in flight, which the sanitized hook environment no
/// longer provides for free.
pub(crate) fn staged_or_worktree<'r>(
    view: &View<'r>,
    workspace_root: &Path,
    file: &str,
    source: &str,
) -> Option<&'r str> {
    if source == "staged" {
        // `:./` resolves the path against the current directory. The bare
        // `:{file}` form resolves from the repository root, so from a mock
        // directory it reads `<repo>/crates/...` rather than
        // `<repo>/mock/crates/...`: the wrong blob where both exist, and a
        // silent fall-through to the worktree where they do not.
        let out = view.git_in(workspace_root, &["show", &format!(":./{file}")]);
        if out.ok() {
            return out.text();
        }
    }
    view.text(workspace_root.join(file))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// The gate end to end against a project that does not use the word
    /// `crates`, which is the case the whole `src_layout` change exists for.
    ///
    /// This walks the real path rather than the predicates: a `RepoContext`
    /// carrying `src_dirs`, a git repository on disk, a frozen phase, and a
    /// finding that has to name the package. The control is the same repository
    /// read through a layout that still says `crates`, where the gate must find
    /// nothing at all, because finding nothing is exactly what shipped before
    /// this and is the failure nobody would have seen.
    #[test]
    fn a_project_that_renamed_its_source_directory_is_gated_where_its_source_is() {
        let root = std::env::temp_dir().join(format!("clr-renamed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("libs/widget/src")).unwrap();
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        // both changelists locked, so the phase is CLOSED and no source may move
        for name in ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.lock.md"] {
            std::fs::write(root.join("design_rounds").join(name), "locked\n").unwrap();
        }
        std::fs::write(root.join("libs/widget/src/lib.rs"), "pub fn a() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["add", "-A"],
            vec!["commit", "-q", "-m", "seed", "--no-gpg-sign"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .unwrap();
        }
        std::fs::write(
            root.join("libs/widget/src/lib.rs"),
            "pub fn a() {}\npub fn b() {}\n",
        )
        .unwrap();

        let run = |dirs: &[std::path::PathBuf]| {
            let crates = std::collections::BTreeSet::new();
            crate::testkit::check_repo(&ChangelistRequired, crate::RepoInput {
                mock_dir:    &root,
                repo_root:   &root,
                all_crates:  &crates,
                src_dirs:    dirs,
                invocation:  None,
                canon_paths: &[],
                open_panels: &[],
                ref_files: &crate::src_layout::NO_REF_FILES,
                registry:    &Default::default(),
            })
        };

        let found = run(&[root.join("libs")]);
        assert_eq!(
            found.len(),
            1,
            "expected the edit under libs/ to be gated: {found:?}"
        );
        assert_eq!(found[0].crate_name, "widget");

        // The control, and the reason this test is worth its runtime.
        assert!(
            run(&[root.join("crates")]).is_empty(),
            "a layout naming a directory this project does not have must find \
             nothing, which is what the old hardcoded gate did here"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A mock dir whose round holds the changelists `names`, and one source
    /// directory, `crates`.
    fn round_at(tag: &str, names: &[&str]) -> (std::path::PathBuf, Vec<PathBuf>) {
        let root = std::env::temp_dir().join(format!("clr-edit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        for name in names {
            std::fs::write(root.join("design_rounds").join(name), "x\n").unwrap();
        }
        let src = vec![root.join("crates")];
        (root, src)
    }

    /// [`admits_edit`] over a fresh reading of `root`, as the hook asks it.
    fn admits(root: &std::path::Path, src: &[PathBuf], file: &str, text: &str) -> bool {
        let reading = crate::Reading::new(root);
        admits_edit(&reading.view(), root, src, file, text)
    }

    const DOCS_ONLY: &str = "//! The new crate, in its own words.\n#![no_std]\n";
    const ONE_ITEM: &str = "//! The new crate, in its own words.\n#![no_std]\npub fn a() {}\n";
    const NEW_LIB: &str = "crates/born/src/lib.rs";

    /// The doc phase of a round adding a crate writes the crate's `lib.rs`
    /// holding its documentation only, and the gate passes it. The twin that
    /// differs by one item is refused, so what passes is the scaffold and not
    /// the phase.
    #[test]
    fn the_doc_phase_admits_a_lib_rs_of_documentation_and_refuses_one_item() {
        let (root, src) = round_at("doc", &["202601010000_changelist.doc.md"]);
        let docs = admits(&root, &src, NEW_LIB, DOCS_ONLY);
        let item = admits(&root, &src, NEW_LIB, ONE_ITEM);
        let _ = std::fs::remove_dir_all(&root);
        assert!(docs, "a lib.rs of documentation is the scaffold a doc phase writes");
        assert!(!item, "one item is source, and source waits for the source phase");
    }

    /// The source phase admits the item the doc phase refused.
    #[test]
    fn the_source_phase_admits_the_items() {
        let (root, src) = round_at("src", &[
            "202601010000_changelist.doc.lock.md",
            "202601010001_changelist.src.md",
        ]);
        let item = admits(&root, &src, NEW_LIB, ONE_ITEM);
        let _ = std::fs::remove_dir_all(&root);
        assert!(item);
    }

    /// An item on the same line as an inner attribute is an item. The line
    /// scanner this replaced skipped every line starting `#![`, so both of these
    /// read as declaring nothing and walked past the gate.
    #[test]
    fn an_item_after_an_inner_attribute_on_its_line_is_an_item() {
        assert!(!declares_nothing("#![no_std] pub fn a() {}\n"));
        assert!(!declares_nothing("//! d\n#![doc = \"x\"] mod evil { pub fn f(){} }\n"));
        // The controls: docs, attributes and comments alone.
        assert!(declares_nothing("//! d\n#![no_std]\n#![doc = \"x\"]\n/* c */\n// c\n"));
        assert!(declares_nothing(""));
    }

    /// Text that does not parse is not known to declare nothing.
    #[test]
    fn text_that_does_not_parse_declares_something() {
        assert!(!declares_nothing("//! d\n#![no_std\n"));
    }

    /// A path outside the source directories, or not Rust, is not the gate's.
    #[test]
    fn a_file_the_gate_does_not_hold_is_admitted() {
        let (root, src) = round_at("outside", &["202601010000_changelist.doc.md"]);
        let elsewhere = admits(&root, &src, "tools/x/src/lib.rs", ONE_ITEM);
        let not_rust = admits(&root, &src, "crates/born/build.sh", ONE_ITEM);
        let _ = std::fs::remove_dir_all(&root);
        assert!(elsewhere && not_rust);
    }

    /// Catalogued gap, tracked #41. Under `git commit -a` the commit carries
    /// the worktree blob of a staged-then-modified file, so the content this
    /// function judges should be the worktree's. Today it returns the index
    /// blob (correct for plain `git commit`, stale for `commit -a`), and the
    /// function has no signal for which shape is in flight.
    #[test]
    #[ignore = "catalogue: staged-then-modified file under commit -a commits \
                the worktree blob while this reads the index blob; needs the \
                commit shape signal; tracked #41"]
    fn a_staged_then_modified_file_reads_as_what_commit_a_commits() {
        let dir = std::env::temp_dir().join(format!("clr_commit_a_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crates/x/src")).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&dir)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(&["init", "-q"]);
        let file = "crates/x/src/lib.rs";
        std::fs::write(dir.join(file), "pub fn staged_version() {}\n").unwrap();
        git(&["add", file]);
        // Modified again after staging: this is what `commit -a` would commit.
        std::fs::write(dir.join(file), "pub fn worktree_version() {}\n").unwrap();

        let reading = crate::Reading::new(&dir);
        let content = staged_or_worktree(&reading.view(), &dir, file, "staged").unwrap().to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            content.contains("worktree_version"),
            "under commit -a the judged content must be the worktree blob; \
             got the index blob instead: {content:?}"
        );
    }
}

#[cfg(test)]
#[path = "changelist_required_fmt_tests.rs"]
mod fmt_tests;

#[cfg(test)]
#[path = "changelist_required_gut_tests.rs"]
mod gut_tests;
