//! Lint: export count warning.
//!
//! Warns when a file has more than 5 `pub` items at module root level.
//! Re-exports (`pub use`) and submodules (`pub mod`) are excluded from the
//! count. Items inside `define_*!` macro invocations are also excluded.
//!
//! This is a WARNING lint — it does not hard-block. The orchestrator may
//! choose to treat it differently from error-level lints.

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const MAX_EXPORTS: usize = 5;

/// Item kinds that count as public exports when they have `pub` visibility.
const COUNTABLE_KINDS: &[&str] = &[
    "struct_item",
    "enum_item",
    "trait_item",
    "function_item",
    "type_item",
    "const_item",
    "static_item",
    "impl_item",
];

pub struct ExportCount;

impl Lint for ExportCount {
    fn default_severity(&self) -> crate::Severity { crate::Severity::ADVISORY }
    fn name(&self) -> &'static str {
        "export-count"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let root = ctx.tree.root_node();
        let count = count_pub_exports(root, ctx.source);

        if count > MAX_EXPORTS {
            vec![LintError {
                crate_name: ctx.crate_name.to_string(),
                line: 1,
                lint_name: "export-count",
                severity: crate::Severity::ADVISORY,
                message: format!(
                    "file has {count} public exports (guideline: ~{MAX_EXPORTS})",
                ),
            }]
        } else {
            Vec::new()
        }
    }
}

fn count_pub_exports(root: Node, source: &str) -> usize {
    let mut count = 0;
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        // Skip use_declaration (pub use) and mod_item (pub mod)
        if child.kind() == "use_declaration" || child.kind() == "mod_item" {
            continue;
        }

        // Skip items inside macro invocations (define_*! calls)
        if child.kind() == "macro_invocation" {
            continue;
        }

        if !COUNTABLE_KINDS.contains(&child.kind()) {
            continue;
        }

        if has_pub_visibility(child, source) {
            count += 1;
        }
    }

    count
}

fn has_pub_visibility(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = &source[child.byte_range()];
            // Match `pub` but not `pub(crate)` or `pub(super)` etc.
            // All of these still count as public exports from the file's
            // perspective — the guideline is about cognitive load, not
            // strict API surface.
            return text.starts_with("pub");
        }
    }
    false
}
