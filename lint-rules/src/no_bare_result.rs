//! Lint: no bare `Result<T, E>` with non-LoimuError error types.
//!
//! Functions should use `Result<T, LoimuError>` for error returns, or
//! `#[optimize_for(hot|cold)]` to get `Outcome<T>`/`Just<T>` rewrites.
//!
//! Allowed: `Result<T, LoimuError>`, `Outcome<T>`, `Just<T>`.
//! Also allowed: `fmt::Result`, `io::Result`, `Result<T, Box<dyn Error>>`.
//!
//! Exempt: crates that precede the error-primitives crate in the dependency tree.

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const BLESSED_TYPES: &[&str] = &[
    "Outcome",
    "Just",
];

pub struct NoBareResult;

impl Lint for NoBareResult {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        "no-bare-result"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_fns(root, ctx, &mut errors);
        errors
    }
}

fn visit_fns(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" | "function_signature_item" => {
                check_fn_return(child, ctx, errors);
            }
            _ => {
                if child.named_child_count() > 0 {
                    visit_fns(child, ctx, errors);
                }
            }
        }
    }
}

fn check_fn_return(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    if has_optimize_for_attr(node, ctx.source) {
        return;
    }

    let ret_node = match node.child_by_field_name("return_type") {
        Some(n) => n,
        None => return,
    };

    let ret_text = txt(ret_node, ctx.source).trim();
    let ret_text = ret_text.strip_prefix("->").unwrap_or(ret_text).trim();

    if ret_text == "fmt::Result"
        || ret_text == "std::fmt::Result"
        || ret_text.starts_with("io::Result")
        || ret_text.starts_with("std::io::Result")
    {
        return;
    }

    for ty in BLESSED_TYPES {
        if ret_text.starts_with(ty) {
            return;
        }
    }

    // Result<T, LoimuError> is the standard error return
    if ret_text.starts_with("Result<") && ret_text.contains("LoimuError") {
        return;
    }

    // Result<T, Box<dyn Error>> is allowed for external-facing code
    if ret_text.starts_with("Result<") && ret_text.contains("Box<dyn") {
        return;
    }

    if ret_text.starts_with("Result<") {
        let fn_name = node
            .child_by_field_name("name")
            .map(|n| txt(n, ctx.source))
            .unwrap_or("<unknown>");

        // Check for lint:allow(no_bare_result) on the line
        let line_idx = node.start_position().row;
        let line = ctx.source.lines().nth(line_idx).unwrap_or("");
        if line.contains("lint:allow(no_bare_result)") {
            errors.push(LintError::warning(
                ctx.crate_name.to_string(),
                line_idx + 1,
                "no-bare-result",
                format!("suppressed by lint:allow(no_bare_result) on fn `{fn_name}` — review periodically"),
            ));
        } else {
            errors.push(LintError {
                crate_name: ctx.crate_name.to_string(),
                line: line_idx + 1,
                lint_name: "no-bare-result",
                severity: crate::Severity::HARD_ERROR,
                message: format!(
                    "fn `{fn_name}` returns `{ret_text}` — use Result<T, LoimuError> or #[optimize_for(hot|cold)]",
                ),
                finding_kind: None,
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

/// Check if a function has an `#[optimize_for(...)]` attribute.
/// The macro rewrites the return type, so bare `Result` is expected in source.
///
/// In tree-sitter, attributes are siblings preceding the function node,
/// not children of it.
fn has_optimize_for_attr(node: Node, source: &str) -> bool {
    let mut prev = node.prev_sibling();
    while let Some(sib) = prev {
        if sib.kind() == "attribute_item" {
            let text = txt(sib, source);
            if text.contains("optimize_for") {
                return true;
            }
        } else if sib.kind() != "line_comment" && sib.kind() != "block_comment" {
            break;
        }
        prev = sib.prev_sibling();
    }
    false
}

fn txt<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}
