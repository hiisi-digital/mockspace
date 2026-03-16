//! Lint: all `define_*!` macros must generate `Registrable` impls.
//!
//! The `Registrable` trait is the foundation of the registry system. Every
//! framework type created by a `define_*!` macro must implement it so that
//! static discovery via `inventory` works. This lint checks macro_rules!
//! definitions to ensure they include `Registrable` in their expansion.
//!
//! Severity: Error (blocks commit, build, and push).
//!
//! Exempt macros: define_id, define_handle, define_error, define_raw_error,
//! define_record, define_registry, define_resource (handled by storage layer),
//! and helper macros (names starting with `__`).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "registrable-completeness";

/// Macros that define framework types but are NOT expected to generate
/// Registrable impls (infrastructure, IDs, errors, records).
const EXEMPT_MACROS: &[&str] = &[
    "define_id",
    "define_handle",
    "define_error",
    "define_raw_error",
    "define_record",
    "define_registry",
    "define_resource",       // generates StorageRecord, not Registrable directly
    "define_resource_inline",
];

pub struct RegistrableCompleteness;

impl Lint for RegistrableCompleteness {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if crate::PROC_MACRO_CRATES.contains(&ctx.crate_name) {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_macros(root, ctx, &mut errors);
        errors
    }
}

fn visit_macros(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "macro_definition" {
            check_macro_def(child, ctx, errors);
        }
        if child.named_child_count() > 0 {
            visit_macros(child, ctx, errors);
        }
    }
}

fn check_macro_def(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };

    let macro_name = &ctx.source[name_node.byte_range()];

    // only check define_* macros
    if !macro_name.starts_with("define_") {
        return;
    }

    // skip helper macros (internal, prefixed with __)
    if macro_name.starts_with("__") {
        return;
    }

    // skip exempt macros
    if EXEMPT_MACROS.contains(&macro_name) {
        return;
    }

    // check for lint:allow suppression
    let line_idx = node.start_position().row;
    if line_idx > 0 {
        if let Some(prev_line) = ctx.source.lines().nth(line_idx - 1) {
            if prev_line.contains("lint:allow(registrable_completeness)") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    LINT_NAME,
                    format!(
                        "`{macro_name}!` suppressed by lint:allow — review periodically",
                    ),
                ));
                return;
            }
        }
    }

    // get the full macro body text
    let body_text = &ctx.source[node.byte_range()];

    // check that the macro body generates Registrable impl
    let has_registrable = body_text.contains("Registrable")
        || body_text.contains("registrable");

    if !has_registrable {
        errors.push(LintError {
            crate_name: ctx.crate_name.to_string(),
            line: line_idx + 1,
            lint_name: LINT_NAME,
            severity: crate::Severity::HARD_ERROR,
            message: format!(
                "`{macro_name}!` does not generate `impl Registrable` — \
                 every define_*! macro must produce a Registrable impl + inventory::submit!",
            ),
            finding_kind: None,
        });
    }
}
