//! Lint: no bare construction of types that should come from macros.
//!
//! Framework types like `LoimuError`, `Diagnostic`, `ErrorDescriptor`,
//! `ErrorCode` have constructors (`::new()`, `::from()`) that should only
//! be called from their defining crate or from within macro expansions.
//!
//! Domain crates must use `define_error!` / `define_warning!` / `define_hint!`
//! to create error types, then call `TypeName::error(...)` or
//! `TypeName::diagnostic(...)` on those generated types.
//!
//! This catches:
//! - `LoimuError::new(...)` / `LoimuError::from(...)`
//! - `Diagnostic::new(...)`
//! - `ErrorDescriptor { ... }` (struct literal)
//! - `ErrorCode::new(...)`

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

/// (type_or_call pattern, crate suffixes that are allowed to use it)
/// At check time, full crate names are built as `<prefix>-<suffix>`.
const FORBIDDEN_CALLS: &[(&str, &[&str])] = &[
    ("LoimuError::new", &["error-primitives"]),
    ("LoimuError::from", &["error-primitives", "diagnostics"]),
    ("Diagnostic::new", &["error-primitives"]),
    ("ErrorCode::new", &["error-primitives", "id"]),
    // Descriptor struct literals (all sealed, but catch any leaks)
    ("ErrorDescriptor", &["diagnostics"]),
    ("SignalDescriptor", &["signal"]),
    ("ResourceDescriptor", &["resource"]),
    ("BehaviorDescriptor", &["behavior"]),
    ("ActionDescriptor", &["action"]),
    ("ProviderDescriptor", &["provider"]),
    ("ModuleDescriptor", &["module"]),
    ("MarkerDescriptor", &["tree"]),
    ("BlueprintDescriptor", &["tree"]),
    ("ScopeDescriptor", &["scope"]),
    ("PersistenceConfig", &["storage"]),
];

pub struct NoBareMacroTypes;

impl Lint for NoBareMacroTypes {
    fn name(&self) -> &'static str {
        "no-bare-macro-types"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();
        visit_nodes(ctx.tree.root_node(), ctx, &mut errors);
        errors
    }
}

fn visit_nodes(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "call_expression" => check_call(child, ctx, errors),
            "struct_expression" => check_struct_literal(child, ctx, errors),
            _ => {}
        }
        if child.named_child_count() > 0 {
            visit_nodes(child, ctx, errors);
        }
    }
}

fn check_call(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    let func = match node.child_by_field_name("function") {
        Some(n) => n,
        None => return,
    };

    let text = txt(func, ctx.source);

    for &(pattern, exempt_suffixes) in FORBIDDEN_CALLS {
        let is_exempt = exempt_suffixes.iter().any(|s| {
            let full = format!("{}-{}", ctx.crate_prefix, s);
            ctx.crate_name == full
        });
        if is_exempt {
            continue;
        }
        if text == pattern || text.ends_with(&format!("::{}", pattern)) {
            let line_idx = node.start_position().row;
            let line = ctx.source.lines().nth(line_idx).unwrap_or("");
            if line.contains("lint:allow(no_bare_macro_types)") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    "no-bare-macro-types",
                    format!("suppressed by lint:allow(no_bare_macro_types) on `{text}(...)` — review periodically"),
                ));
            } else {
                errors.push(LintError {
                    crate_name: ctx.crate_name.to_string(),
                    line: line_idx + 1,
                    lint_name: "no-bare-macro-types",
                    severity: crate::Severity::HARD_ERROR,
                    message: format!(
                        "bare `{text}(...)` — use define_error!/define_warning!/define_hint! types instead",
                    ),
                });
            }
        }
    }
}

fn check_struct_literal(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    let name_node = match node.child_by_field_name("name") {
        Some(n) => n,
        None => return,
    };

    let text = txt(name_node, ctx.source);
    let bare_name = text.rsplit("::").next().unwrap_or(text);

    for &(pattern, exempt_suffixes) in FORBIDDEN_CALLS {
        let is_exempt = exempt_suffixes.iter().any(|s| {
            let full = format!("{}-{}", ctx.crate_prefix, s);
            ctx.crate_name == full
        });
        if is_exempt {
            continue;
        }
        // Only match struct literal patterns (no :: in pattern = struct name)
        if !pattern.contains("::") && bare_name == pattern {
            let line_idx = node.start_position().row;
            let line = ctx.source.lines().nth(line_idx).unwrap_or("");
            if line.contains("lint:allow(no_bare_macro_types)") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    "no-bare-macro-types",
                    format!("suppressed by lint:allow(no_bare_macro_types) on `{text} {{ ... }}` — review periodically"),
                ));
            } else {
                errors.push(LintError {
                    crate_name: ctx.crate_name.to_string(),
                    line: line_idx + 1,
                    lint_name: "no-bare-macro-types",
                    severity: crate::Severity::HARD_ERROR,
                    message: format!(
                        "bare `{text} {{ ... }}` literal — this type should only be constructed via define_*! macros",
                    ),
                });
            }
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
