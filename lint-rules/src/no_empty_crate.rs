//! Lint: no empty crates.
//!
//! Flags crates whose `src/lib.rs` contains only doc comments, use statements,
//! and/or `mod` declarations but no type definitions, trait definitions, macro
//! invocations, or function definitions.
//!
//! Pre-commit: WARNING. Pre-push / build: ERROR.
//! (Severity is WARNING so it doesn't block incremental work, but the crate
//! must be populated before pushing.)

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "no-empty-crate";

/// Node kinds that count as "substantive content" — having at least one means
/// the crate is not empty.
const SUBSTANTIVE_KINDS: &[&str] = &[
    "struct_item",
    "enum_item",
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
        let root = ctx.tree.root_node();

        if has_substantive_content(root) {
            return Vec::new();
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
