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

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError, make_parser};

const LINT_NAME: &str = "no-empty-crate";

/// Node kinds that count as "substantive content" — having at least one means
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
    fn default_severity(&self) -> crate::Severity { crate::Severity::ADVISORY }
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        // lib.rs tree is already parsed — check it first so the common case
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
            // Skip lib.rs — already checked via ctx.tree above.
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

        vec![LintError::warning(
            ctx.crate_name.to_string(),
            1,
            LINT_NAME,
            format!(
                "crate `{}` has no type, trait, function, or macro definitions — \
                 populate it or remove it from the workspace",
                ctx.crate_name,
            ),
        )]
    }
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
    use super::*;
    use crate::CrateSourceFile;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn make_ctx(
        lib_src: &str,
        extra: &[(&str, &str)],
    ) -> (tree_sitter::Tree, LintContext<'static>) {
        let mut parser = make_parser();
        let tree = parser.parse(lib_src, None).unwrap();
        let tree = Box::leak(Box::new(tree));
        let mut sources: Vec<CrateSourceFile> = vec![CrateSourceFile {
            rel_path: PathBuf::from("src/lib.rs"),
            text: lib_src.to_string(),
        }];
        for (name, body) in extra {
            sources.push(CrateSourceFile {
                rel_path: PathBuf::from(format!("src/{}", name)),
                text: body.to_string(),
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
            workspace_root: Box::leak(Box::new(PathBuf::from("/tmp"))),
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
        // lib.rs only contains mod declarations and re-exports — no direct
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
}
