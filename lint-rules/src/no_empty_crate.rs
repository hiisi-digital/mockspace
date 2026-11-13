//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Lint: no empty crates.
//!
//! Flags crates whose `src/**/*.rs` files contain only doc comments, use
//! statements, and/or `mod` declarations but no type definitions, trait
//! definitions, macro invocations, or function definitions.
//!
//! The crate is judged by the union of all its `.rs` source files, not only
//! `lib.rs`. A crate whose `lib.rs` only does `pub mod x; pub use x::*;`
//! (the common re-export pattern) is NOT empty as long as `x.rs` defines
//! something substantive.
//!
//! Pre-commit: WARNING. Pre-push / build: ERROR.
//! (Severity is WARNING so it doesn't block incremental work, but the crate
//! must be populated before pushing.)
//!
//! A crate a design round is adding is not empty yet, it is unborn. While a
//! round is open and its doc changelist names the crate, the crate's
//! documentation alone passes: the doc phase commits the manifest and a
//! `lib.rs` holding only the crate's docs, which `changelist-required` admits
//! and cargo needs to read the workspace, and the items come in the source
//! phase. Once both changelists are locked the lint holds the crate again, so
//! a round cannot close over a crate it never populated.

use std::path::Path;

use tree_sitter::Node;

use crate::changelist_helpers::{self, Phase};
use crate::{CrateLint, Lint, LintContext, LintError, make_parser};

const LINT_NAME: &str = "no-empty-crate";

/// Node kinds that count as "substantive content": having at least one means
/// the crate is not empty.
const SUBSTANTIVE_KINDS: &[&str] = &[
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "function_item",
    "const_item",
    "static_item",
    "type_item",
    "impl_item",
    "macro_invocation",
    "macro_definition",
];

pub struct NoEmptyCrate;

impl Lint for NoEmptyCrate {
    fn reach(&self) -> crate::Reach {
        crate::Reach::Crate
    }

    /// Crate-scoped. Whether a crate is empty is a fact about the crate; per
    /// file it would call every module file an empty crate.
    fn per_file(&self) -> bool {
        false
    }

    fn default_severity(&self) -> crate::Severity {
        crate::Severity::ADVISORY
    }

    fn name(&self) -> &'static str {
        LINT_NAME
    }
}

impl CrateLint for NoEmptyCrate {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        // lib.rs tree is already parsed: check it first so the common case
        // of a populated lib.rs avoids re-parsing every module.
        if has_substantive_content(ctx.tree.root_node()) {
            return Vec::new();
        }

        // Fall through: scan every other source file (e.g. `src/foo.rs`) for
        // substantive content. Handles the `pub mod foo; pub use foo::*;`
        // re-export pattern where lib.rs itself holds nothing substantive
        // but the crate clearly has an API surface.
        let mut parser = make_parser();
        for file in ctx.all_sources {
            // Skip lib.rs: already checked via ctx.tree above.
            if file.rel_path.file_name().and_then(|n| n.to_str()) == Some("lib.rs") {
                continue;
            }
            let Some(tree) = parser.parse(&file.text, None) else {
                continue;
            };
            if has_substantive_content(tree.root_node()) {
                return Vec::new();
            }
        }

        if born_in_the_open_round(ctx.workspace_root, ctx.crate_name) {
            return Vec::new();
        }

        vec![LintError::warning(
            ctx.crate_name.to_string(),
            1,
            LINT_NAME,
            format!(
                "crate `{}` has no type, trait, function, or macro definitions: \
                 populate it or remove it from the workspace",
                ctx.crate_name,
            ),
        )]
    }
}

/// Whether the round open under `mock_dir` is adding `crate_name`: its phase
/// is DOC, DRAFT or IMPL, and its doc changelist names the crate.
fn born_in_the_open_round(mock_dir: &Path, crate_name: &str) -> bool {
    let rounds = mock_dir.join("design_rounds");
    if !matches!(
        changelist_helpers::current_phase(&rounds),
        Phase::Doc | Phase::SrcPlan | Phase::Src
    ) {
        return false;
    }
    changelist_helpers::find_active_doc_cl(&rounds)
        .or_else(|| changelist_helpers::find_locked_doc_cl(&rounds))
        .and_then(|cl| std::fs::read_to_string(rounds.join(cl.filename)).ok())
        .is_some_and(|text| names_crate(&text, crate_name))
}

/// Whether `text` names `crate_name` as a whole word, so `foo` is not named by
/// a mention of `foo-extra`.
fn names_crate(text: &str, crate_name: &str) -> bool {
    let part_of_a_name = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    text.match_indices(crate_name).any(|(at, _)| {
        let before = text[.. at].chars().next_back();
        let after = text[at + crate_name.len() ..].chars().next();
        !before.is_some_and(part_of_a_name) && !after.is_some_and(part_of_a_name)
    })
}

fn has_substantive_content(node: Node) -> bool {
    if SUBSTANTIVE_KINDS.contains(&node.kind()) {
        return true;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_substantive_content(child) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;
    use crate::CrateSourceFile;

    fn make_ctx(
        lib_src: &str,
        extra: &[(&str, &str)],
    ) -> (tree_sitter::Tree, LintContext<'static>) {
        make_ctx_at(PathBuf::from("/tmp"), lib_src, extra)
    }

    fn make_ctx_at(
        mock_dir: PathBuf,
        lib_src: &str,
        extra: &[(&str, &str)],
    ) -> (tree_sitter::Tree, LintContext<'static>) {
        let mut parser = make_parser();
        let tree = parser.parse(lib_src, None).unwrap();
        let tree = Box::leak(Box::new(tree));
        let mut sources: Vec<CrateSourceFile> = vec![CrateSourceFile {
            rel_path: PathBuf::from("src/lib.rs"),
            text:     lib_src.to_string(),
        }];
        for (name, body) in extra {
            sources.push(CrateSourceFile {
                rel_path: PathBuf::from(format!("src/{}", name)),
                text:     body.to_string(),
            });
        }
        let ctx = LintContext {
            crate_name: "test-crate",
            short_name: "test",
            source: Box::leak(lib_src.to_string().into_boxed_str()),
            tree,
            all_sources: Box::leak(Box::new(sources)),
            deps: Box::leak(Box::new(Vec::new())),
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: Box::leak("".to_string().into_boxed_str()),
            shame_doc: None,
            workspace_root: Box::leak(Box::new(mock_dir)),
            proc_macro_crates: Box::leak(Box::new(Vec::new())),
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
        };
        (unsafe { std::ptr::read(tree as *const _) }, ctx)
    }

    #[test]
    fn populated_lib_rs_passes() {
        let (_t, ctx) = make_ctx("pub struct Foo;", &[]);
        assert!(NoEmptyCrate.check(&ctx).is_empty());
    }

    #[test]
    fn reexport_from_submodule_passes() {
        // lib.rs only contains mod declarations and re-exports: no direct
        // substantive content, but the submodule provides it. This is the
        // common `pub mod foo; pub use foo::*;` pattern the old lint
        // flagged as a false positive.
        let lib = "pub mod inner;\npub use inner::Thing;\n";
        let inner = "pub struct Thing;\n";
        let (_t, ctx) = make_ctx(lib, &[("inner.rs", inner)]);
        assert!(NoEmptyCrate.check(&ctx).is_empty());
    }

    #[test]
    fn truly_empty_crate_still_flags() {
        // lib.rs and the only submodule are both empty shells.
        let lib = "//! only a doc comment\n";
        let inner = "//! nothing here either\n";
        let (_t, ctx) = make_ctx(lib, &[("inner.rs", inner)]);
        let errs = NoEmptyCrate.check(&ctx);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn lib_rs_only_with_docs_flags() {
        let (_t, ctx) = make_ctx("//! just docs\n", &[]);
        assert_eq!(NoEmptyCrate.check(&ctx).len(), 1);
    }

    /// A mock dir whose round holds the changelists `names`, the doc one
    /// carrying `doc_text`.
    fn round(tag: &str, names: &[&str], doc_text: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        for name in names {
            let text = if name.contains(".doc.") { doc_text } else { "x\n" };
            std::fs::write(root.join("design_rounds").join(name), text).unwrap();
        }
        root
    }

    const DOC_OPEN: &[&str] = &["202601010000_changelist.doc.md"];
    const NAMES_IT: &str = "## Changes\n\nA new crate, `test-crate`, holds the thing.\n";

    fn findings_in(root: PathBuf) -> usize {
        let (_t, ctx) = make_ctx_at(root.clone(), "//! The crate's documentation.\n", &[]);
        let n = NoEmptyCrate.check(&ctx).len();
        let _ = std::fs::remove_dir_all(&root);
        n
    }

    /// The doc phase of a round adding the crate commits its documentation
    /// alone, and the crate is unborn rather than empty.
    #[test]
    fn a_crate_the_open_round_adds_passes_on_its_documentation() {
        assert_eq!(findings_in(round("doc", DOC_OPEN, NAMES_IT)), 0);
    }

    /// Through DRAFT and IMPL as well, since the items land during IMPL.
    #[test]
    fn a_crate_the_round_adds_passes_until_the_source_changelist_locks() {
        let draft = ["202601010000_changelist.doc.lock.md"];
        let imp = ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.md"];
        assert_eq!(findings_in(round("draft", &draft, NAMES_IT)), 0);
        assert_eq!(findings_in(round("impl", &imp, NAMES_IT)), 0);
    }

    /// The control: the same open round, its changelist not naming the crate.
    #[test]
    fn a_round_that_does_not_name_the_crate_does_not_shield_it() {
        let text = "## Changes\n\nNothing about crates.\n";
        assert_eq!(findings_in(round("other", DOC_OPEN, text)), 1);
    }

    /// A name that only contains the crate's name is another crate.
    #[test]
    fn a_longer_name_holding_the_crates_name_is_not_the_crate() {
        let text = "## Changes\n\nA new crate, `test-crate-extra`.\n";
        assert_eq!(findings_in(round("longer", DOC_OPEN, text)), 1);
    }

    /// With both changelists locked the round is closing, and a crate it never
    /// populated is empty.
    #[test]
    fn a_round_with_both_changelists_locked_holds_the_crate_again() {
        let done = ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.lock.md"];
        assert_eq!(findings_in(round("done", &done, NAMES_IT)), 1);
    }
}
