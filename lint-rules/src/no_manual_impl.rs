//! Lint: no hand-written trait impls for framework traits.
//!
//! These traits must only be implemented via macros:
//! Signal, Behavior, Registrable, HasName, Api, Scope, Action.
//!
//! Exempt: crates that define the trait (they contain the macro that generates impls).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

/// (trait_name, crate suffix — impls in `<prefix>-<suffix>` are exempt)
///
/// NOTE: this hardcoded list is a stopgap until #[only_macro_gen] attribute
/// auto-discovery is implemented (Phase 1 of the audit round). After that,
/// the lint will discover forbidden traits by scanning for the attribute.
const FORBIDDEN_IMPLS: &[(&str, &str)] = &[
    ("Signal", "signal"),
    ("Behavior", "behavior"),
    ("Registrable", "registry"),
    ("HasName", "registry"),
    ("Provider", "provider"),
    ("Scope", "scope"),
    ("Action", "action"),
    ("Storage", "storage"),
    ("StorageRecord", "storage"),
    ("HasShape", "buffers"),
    ("Resource", "resource"),
    ("Module", "module"),
    ("Marker", "tree"),
    ("Binding", "gpu"),
    ("GpuSyncable", "gpu"),
];

/// No blanket exemptions for manual impls. Backend crates use
/// `// lint:allow(no_manual_impl)` on specific impl blocks.

pub struct NoManualImpl;

impl Lint for NoManualImpl {
    fn name(&self) -> &'static str {
        "no-manual-impl"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_impls(root, ctx, &mut errors);
        errors
    }
}

fn visit_impls(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "impl_item" {
            check_impl(child, ctx, errors);
        }
        if child.named_child_count() > 0 {
            visit_impls(child, ctx, errors);
        }
    }
}

fn check_impl(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    if is_inside_macro_def(node) {
        return;
    }

    let impl_text = txt(node, ctx.source);

    if !impl_text.contains(" for ") {
        return;
    }

    let after_impl = impl_text.strip_prefix("impl").unwrap_or(impl_text).trim();
    let trait_part = match after_impl.split(" for ").next() {
        Some(t) => t.trim(),
        None => return,
    };

    let trait_name = trait_part
        .rsplit("::")
        .next()
        .unwrap_or(trait_part)
        .trim()
        .split('<')
        .next()
        .unwrap_or("")
        .trim();

    for &(forbidden_trait, defining_suffix) in FORBIDDEN_IMPLS {
        let defining_crate = format!("{}-{}", ctx.crate_prefix, defining_suffix);
        if trait_name == forbidden_trait && ctx.crate_name != defining_crate {
            let type_part = after_impl
                .split(" for ")
                .nth(1)
                .unwrap_or("<unknown>")
                .split('{')
                .next()
                .unwrap_or("<unknown>")
                .trim();

            // Check for lint:allow(no_manual_impl) on the impl line
            let line_idx = node.start_position().row;
            let line = ctx.source.lines().nth(line_idx).unwrap_or("");
            if line.contains("lint:allow(no_manual_impl)") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    "no-manual-impl",
                    format!("suppressed by lint:allow(no_manual_impl) on `impl {forbidden_trait} for {type_part}` — review periodically"),
                ));
            } else {
                errors.push(LintError {
                    crate_name: ctx.crate_name.to_string(),
                    line: line_idx + 1,
                    lint_name: "no-manual-impl",
                    severity: crate::Severity::HARD_ERROR,
                    message: format!(
                        "manual `impl {forbidden_trait} for {type_part}` — use the appropriate define_*! macro",
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
