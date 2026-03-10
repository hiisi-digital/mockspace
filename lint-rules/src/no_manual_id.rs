//! Lint: no manual ID struct definitions.
//!
//! All ID types must be created via `define_id!`. Manual `struct *Id` definitions
//! are forbidden outside the ID infrastructure crate.

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

pub struct NoManualId;

impl Lint for NoManualId {
    fn name(&self) -> &'static str {
        "no-manual-id"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_structs(root, ctx, &mut errors);
        errors
    }
}

fn visit_structs(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "struct_item" {
            check_struct(child, ctx, errors);
        }
        if child.named_child_count() > 0 {
            visit_structs(child, ctx, errors);
        }
    }
}

fn check_struct(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    let name = match node.child_by_field_name("name") {
        Some(n) => txt(n, ctx.source),
        None => return,
    };

    if name.len() > 2 && name.ends_with("Id") {
        // Check for lint:allow(no_manual_id) on the line
        let line_idx = node.start_position().row;
        let line = ctx.source.lines().nth(line_idx).unwrap_or("");
        if line.contains("lint:allow(no_manual_id)") {
            errors.push(LintError::warning(
                ctx.crate_name.to_string(),
                line_idx + 1,
                "no-manual-id",
                format!("suppressed by lint:allow(no_manual_id) on `{name}` — review periodically"),
            ));
        } else {
            errors.push(LintError {
                crate_name: ctx.crate_name.to_string(),
                line: line_idx + 1,
                lint_name: "no-manual-id",
                severity: crate::Severity::HARD_ERROR,
                message: format!(
                    "manual ID struct `{name}` — use define_id!({name}) or define_handle!({name}) from the ID crate",
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
