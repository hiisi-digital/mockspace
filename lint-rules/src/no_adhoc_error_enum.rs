//! Lint: no ad-hoc error enums.
//!
//! All error types must be defined via `define_error!`, `define_warning!`,
//! `define_hint!`, or `define_raw_error!`. Manual `enum *Error { ... }`
//! definitions are forbidden.
//!
//! Exempt: the error-primitives crate (defines the error infrastructure itself).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

pub struct NoAdhocErrorEnum;

impl Lint for NoAdhocErrorEnum {
    fn name(&self) -> &'static str {
        "no-adhoc-error-enum"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_enums(root, ctx, &mut errors);
        errors
    }
}

fn visit_enums(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "enum_item" {
            check_enum(child, ctx, errors);
        }
        if child.named_child_count() > 0 {
            visit_enums(child, ctx, errors);
        }
    }
}

fn check_enum(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    let name = match node.child_by_field_name("name") {
        Some(n) => txt(n, ctx.source),
        None => return,
    };

    if name.ends_with("Error") {
        let line_idx = node.start_position().row;
        let line = ctx.source.lines().nth(line_idx).unwrap_or("");
        if line.contains("lint:allow(no_adhoc_error_enum)") {
            errors.push(LintError::warning(
                ctx.crate_name.to_string(),
                line_idx + 1,
                "no-adhoc-error-enum",
                format!("suppressed by lint:allow(no_adhoc_error_enum) on `{name}` — review periodically"),
            ));
        } else {
            errors.push(LintError {
                crate_name: ctx.crate_name.to_string(),
                line: line_idx + 1,
                lint_name: "no-adhoc-error-enum",
                severity: crate::Severity::HARD_ERROR,
                message: format!(
                    "ad-hoc error enum `{name}` — use define_error!, define_warning!, or define_raw_error!",
                ),
            });
        }
    }
}

fn is_inside_macro_def(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "macro_definition" {
            return true;
        }
        current = parent.parent();
    }
    false
}

fn txt<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}
