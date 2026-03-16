//! Lint: no bare `String` in struct/enum fields.
//!
//! Framework internals should use interned IDs (`define_id!` types) instead of
//! `String` for identifiers. `&'static str` is fine for compile-time constants
//! and descriptor metadata.
//!
//! Severity: PushError (warning on commit, error on push).
//!
//! No crate-level exemptions. Every suppression must be at the use site with
//! `// lint:allow(no_bare_string) — <explanation>`. Unexplained suppressions
//! are PushError; explained suppressions (8+ words) are Warning.
//!
//! Fields inside `macro_rules!` definitions are skipped (generated code).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

pub struct NoBareString;

impl Lint for NoBareString {
    fn default_severity(&self) -> crate::Severity { crate::Severity::PUSH_GATE }
    fn name(&self) -> &'static str {
        "no-bare-string"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_nodes(root, ctx, &mut errors);
        errors
    }
}

fn visit_nodes(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "struct_item" | "enum_item" => {
                if !is_inside_macro_def(child) {
                    check_type_fields(child, ctx, errors);
                }
            }
            _ => {}
        }
        if child.named_child_count() > 0 {
            visit_nodes(child, ctx, errors);
        }
    }
}

fn check_type_fields(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check_field_recursive(child, ctx, errors);
    }
}

fn check_field_recursive(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if node.kind() == "field_declaration" {
        let line_idx = node.start_position().row;
        let field_text = txt(node, ctx.source);

        if !contains_bare_string_type(field_text) {
            // No String type in this field — skip.
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                check_field_recursive(child, ctx, errors);
            }
            return;
        }

        // String type found. Check for suppression on this line or up to 3 preceding lines.
        let context = gather_context_lines(ctx.source, line_idx, 3);
        if let Some(explanation) = extract_allow_explanation(&context, "no_bare_string") {
            if explanation.split_whitespace().count() >= 8 {
                // Explained suppression → Warning (never blocks).
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    "no-bare-string",
                    format!("suppressed: `String` field — {explanation}"),
                ));
            } else {
                // Unexplained suppression → PushError (must explain!).
                errors.push(LintError::push_error(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    "no-bare-string",
                    "`String` field suppressed without explanation (need 8+ words after `—`)".to_string(),
                ));
            }
            return;
        }

        // No suppression — report violation.
        errors.push(LintError::push_error(
            ctx.crate_name.to_string(),
            line_idx + 1,
            "no-bare-string",
            "`String` field in struct/enum — use a `define_id!` type or `&'static str` for compile-time names".to_string(),
        ));
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check_field_recursive(child, ctx, errors);
    }
}

/// Check if a field declaration contains a bare `String` type.
///
/// Matches patterns like:
/// - `pub name: String`
/// - `pub names: DenseColumn<String>`
/// - `name: Option<String>`
///
/// Does NOT match:
/// - `pub name: &'static str` (compile-time constant, fine)
fn contains_bare_string_type(field_text: &str) -> bool {
    // Look for the type annotation part (after the colon).
    let type_part = match field_text.split(':').nth(1) {
        Some(t) => t.trim(),
        None => return false,
    };

    // Check for String as standalone type or as generic parameter.
    if type_part == "String"
        || type_part == "String,"
        || type_part.starts_with("String,")
        || type_part.starts_with("String>")
    {
        return true;
    }

    // Check inside generic parameters: DenseColumn<String>, Option<String>, etc.
    if type_part.contains("<String>")
        || type_part.contains("<String,")
        || type_part.contains(", String>")
        || type_part.contains(", String,")
    {
        return true;
    }

    false
}

/// Gather the target line plus up to `look_back` preceding lines into one string.
fn gather_context_lines(source: &str, target_line: usize, look_back: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = target_line.saturating_sub(look_back);
    lines[start..=target_line.min(lines.len() - 1)].join("\n")
}

/// Extract the explanation text after `lint:allow(rule_name) —`.
/// Returns None if no suppression marker is found.
/// Returns Some("") if marker exists but no explanation after the dash.
fn extract_allow_explanation<'a>(context: &'a str, rule_name: &str) -> Option<&'a str> {
    let marker = format!("lint:allow({rule_name})");
    let pos = context.find(&marker)?;
    let after = &context[pos + marker.len()..];

    // Take only up to the end of the line containing the marker.
    let rest = after.split('\n').next().unwrap_or(after);

    // Look for ` — ` (em dash) or ` - ` (hyphen) as separator.
    if let Some(dash_pos) = rest.find('—') {
        Some(rest[dash_pos + '—'.len_utf8()..].trim())
    } else if let Some(dash_pos) = rest.find(" - ") {
        Some(rest[dash_pos + 3..].trim())
    } else {
        Some("") // Marker found but no explanation.
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
