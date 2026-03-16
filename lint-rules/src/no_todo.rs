//! Lint: no `todo!()` in mock source files.
//!
//! The mockspace is production code. Every function must have a real
//! implementation. `todo!()` is not acceptable.
//!
//! Severity: PushError (warning on commit, error on push).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "no-todo";

pub struct NoTodo;

impl Lint for NoTodo {
    fn default_severity(&self) -> crate::Severity { crate::Severity::PUSH_GATE }
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        find_todo_macros(root, ctx, &mut errors);
        errors
    }
}

fn find_todo_macros(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if node.kind() == "macro_invocation" {
        if let Some(name_node) = node.child_by_field_name("macro") {
            let name = &ctx.source[name_node.byte_range()];
            if name == "todo" || name == "todo!" {
                errors.push(LintError::push_error(
                    ctx.crate_name.to_string(),
                    node.start_position().row + 1,
                    LINT_NAME,
                    "todo!() is not allowed — implement fully".to_string(),
                ));
                return;
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_todo_macros(child, ctx, errors);
    }
}
