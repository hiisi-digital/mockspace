//! Lint: macro-defining crates must not call their own macros.
//!
//! Infrastructure crates provide `define_*!` macro facilities for domain crates.
//! They must never call those macros themselves. For example, the ID crate provides
//! `define_id!` but must not call it — domain crates call
//! `define_id!(SignalId)` in their own source.
//!
//! This applies to BOTH the crate that defines the macro AND any crate that
//! re-exports it. If the resource crate does `pub use <prefix>_storage::define_resource`,
//! then the resource crate also must not call `define_resource!`.
//!
//! Proc-macro crates (`<prefix>-*-macros`) are checked too — they should not call
//! their own `define_*` proc macros. But only `define_*` macros are checked;
//! other proc macros (derive macros, attribute macros) are not a concern.
//!
//! Severity: Error (blocks commit and push).
//!
//! Suppression: `// lint:allow(no_self_define) — <explanation>` requires a 50+
//! word explanation documenting why this invocation must live in the defining
//! crate. Unexplained or too-short suppressions remain Error.
//!
//! Exemption: invocations inside `macro_rules!` bodies are skipped (the macro
//! definition itself obviously contains the macro name).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "no-self-define";

/// Maps each `define_*` macro name to the crate suffix(es) that define or
/// re-export it. At check time, full crate names are built as
/// `<prefix>-<suffix>`. Every matching crate must NOT call this macro
/// (except inside its own `macro_rules!` definition body).
///
/// Format: (macro_name, &[defining_suffix, ...re-exporting_suffixes])
const MACRO_OWNERS: &[(&str, &[&str])] = &[
    ("define_id", &["id"]),
    ("define_signal", &["signal"]),
    ("define_behavior", &["behavior"]),
    ("define_projection", &["behavior"]),
    ("define_registry", &["registry"]),
    ("define_action", &["registry"]),
    ("define_blueprint", &["registry"]),
    ("define_marker", &["tree"]),
    ("define_scope", &["scope"]),
    ("define_module", &["module"]),
    ("define_plugin", &["plugin"]),
    ("define_binding", &["gpu"]),
    ("define_record", &["storage"]),
    ("define_resource", &["storage", "resource"]),
    ("define_storage", &["storage"]),
    ("define_error", &["diagnostics"]),
    ("define_warning", &["diagnostics"]),
    ("define_hint", &["diagnostics"]),
    ("define_raw_error", &["error-primitives"]),
    ("define_provider", &["provider"]),
];

pub struct NoSelfDefine;

impl Lint for NoSelfDefine {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        // Find which macros this crate owns or re-exports.
        let owned: Vec<&str> = MACRO_OWNERS
            .iter()
            .filter(|(_, suffixes)| {
                suffixes.iter().any(|s| {
                    let full = format!("{}-{}", ctx.crate_prefix, s);
                    ctx.crate_name == full
                })
            })
            .map(|(macro_name, _)| *macro_name)
            .collect();

        if owned.is_empty() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        find_self_invocations(root, ctx, &owned, &mut errors);
        errors
    }
}

fn find_self_invocations(
    node: Node,
    ctx: &LintContext,
    owned_macros: &[&str],
    errors: &mut Vec<LintError>,
) {
    if node.kind() == "macro_invocation" {
        // Skip if inside a macro_rules! definition body.
        if is_inside_macro_def(node) {
            return;
        }

        if let Some(name_node) = node.child_by_field_name("macro") {
            let raw_name = &ctx.source[name_node.byte_range()];
            // Handle both `define_foo!` and `<prefix>_storage::define_foo!` forms.
            let clean = extract_macro_name(raw_name);
            if owned_macros.contains(&clean) {
                let line_idx = node.start_position().row;
                let line = ctx.source.lines().nth(line_idx).unwrap_or("");

                // Check for lint:allow suppression with 50+ word explanation.
                // Large look-back because 50+ word explanations span many comment lines.
                let context = gather_context_lines(ctx.source, line_idx, 15);
                if let Some(explanation) = extract_allow_explanation(&context, "no_self_define") {
                    let word_count = explanation.split_whitespace().count();
                    if word_count >= 50 {
                        errors.push(LintError::warning(
                            ctx.crate_name.to_string(),
                            line_idx + 1,
                            LINT_NAME,
                            format!(
                                "suppressed ({word_count} words): `{clean}!` in `{}` — {explanation}",
                                ctx.crate_name,
                            ),
                        ));
                    } else {
                        errors.push(LintError::error(
                            ctx.crate_name.to_string(),
                            line_idx + 1,
                            LINT_NAME,
                            format!(
                                "`{clean}!` suppression in `{}` too short ({word_count}/50 words) \
                                 — explain in depth why this must live here, not in a domain crate",
                                ctx.crate_name,
                            ),
                        ));
                    }
                } else {
                    errors.push(LintError::error(
                        ctx.crate_name.to_string(),
                        line_idx + 1,
                        LINT_NAME,
                        format!(
                            "`{clean}!` must not be called in `{}` (defines or re-exports it) \
                             — domain crates call it, not the infrastructure crate",
                            ctx.crate_name,
                        ),
                    ));
                }
            }
        }
        return;
    }

    // Skip macro_definition children entirely (the body of macro_rules!).
    if node.kind() == "macro_definition" {
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_self_invocations(child, ctx, owned_macros, errors);
    }
}

/// Extract the bare macro name from potentially qualified invocations.
/// `define_id` → `define_id`
/// `define_id!` → `define_id`
/// `<prefix>_storage::define_resource!` → `define_resource`
/// `<prefix>_storage::define_resource` → `define_resource`
fn extract_macro_name(raw: &str) -> &str {
    let without_bang = raw.trim_end_matches('!');
    // Take the last segment after `::`.
    match without_bang.rsplit("::").next() {
        Some(name) => name,
        None => without_bang,
    }
}

/// Gather the target line plus up to `look_back` preceding lines into one string.
fn gather_context_lines(source: &str, target_line: usize, look_back: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = target_line.saturating_sub(look_back);
    lines[start..=target_line.min(lines.len().saturating_sub(1))].join("\n")
}

/// Extract the explanation text after `lint:allow(rule_name) —`.
/// Returns None if no suppression marker is found.
/// Returns Some("") if marker exists but no explanation after the dash.
///
/// For multi-line `//` comments, collects all continuation lines (lines
/// starting with `//`) after the marker line until a non-comment line.
fn extract_allow_explanation(context: &str, rule_name: &str) -> Option<String> {
    let marker = format!("lint:allow({rule_name})");
    let pos = context.find(&marker)?;
    let after = &context[pos + marker.len()..];

    // Find the separator (em dash or hyphen).
    let rest = if let Some(dash_pos) = after.find('—') {
        &after[dash_pos + '—'.len_utf8()..]
    } else if let Some(dash_pos) = after.find(" - ") {
        &after[dash_pos + 3..]
    } else {
        return Some(String::new()); // Marker found but no explanation.
    };

    // Collect the first line + continuation comment lines.
    let mut parts = Vec::new();
    for line in rest.lines() {
        let trimmed = line.trim();
        if parts.is_empty() {
            // First line (rest of the marker line).
            parts.push(trimmed.to_string());
        } else if let Some(comment_text) = trimmed.strip_prefix("//") {
            // Continuation comment line.
            parts.push(comment_text.trim().to_string());
        } else {
            // Non-comment line: stop collecting.
            break;
        }
    }

    Some(parts.join(" ").trim().to_string())
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
