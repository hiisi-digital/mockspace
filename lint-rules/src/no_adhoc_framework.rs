//! Lint: no ad-hoc framework pattern reimplementations.
//!
//! Heuristic detection of manually-built registries, markers, records, and
//! other framework patterns that should use `define_*!` macros. Complements
//! `no-manual-impl` (which catches trait impls) by catching structural
//! patterns where someone builds the same thing without the trait.
//!
//! No crate-level exemptions. Every suppression must be at the use site with
//! `// lint:allow(no_adhoc_framework) — <explanation>`. Unexplained
//! suppressions are PushError; explained suppressions (8+ words) are Warning.
//!
//! Heuristics:
//! 1. Struct named `*Registry` outside `define_registry!` or `macro_rules!` → Error
//! 2. Direct `inventory::submit!` / `inventory::collect!` outside `macro_rules!` → Error
//! 3. `OnceLock<Mutex<` or `OnceLock<RwLock<` pattern (global mutable singleton) → Warning
//! 4. `HashMap<&str, usize>` or `HashMap<String, usize>` (index-lookup map) → Error

use crate::{Lint, LintContext, LintError};

pub struct NoAdhocFramework;

impl Lint for NoAdhocFramework {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        "no-adhoc-framework"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();

        let mut in_macro_def = false;
        let mut macro_brace_depth: i32 = 0;

        // Track define_registry! invocations so we don't flag their generated body.
        let mut in_define_registry = false;
        let mut registry_brace_depth: i32 = 0;

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments.
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }

            // Track macro_rules! definitions.
            if trimmed.contains("macro_rules!") {
                in_macro_def = true;
                macro_brace_depth = 0;
            }
            if in_macro_def {
                macro_brace_depth += trimmed.chars().filter(|c| *c == '{').count() as i32;
                macro_brace_depth -= trimmed.chars().filter(|c| *c == '}').count() as i32;
                if macro_brace_depth <= 0 && !trimmed.contains("macro_rules!") {
                    in_macro_def = false;
                }
                continue; // Inside macro_rules! body — skip all checks.
            }

            // Track define_registry! invocations.
            if trimmed.contains("define_registry!") {
                in_define_registry = true;
                registry_brace_depth = 0;
            }
            if in_define_registry {
                registry_brace_depth += trimmed.chars().filter(|c| *c == '{' || *c == '(').count() as i32;
                registry_brace_depth -= trimmed.chars().filter(|c| *c == '}' || *c == ')').count() as i32;
                if registry_brace_depth <= 0 && !trimmed.contains("define_registry!") {
                    in_define_registry = false;
                }
                continue;
            }

            // ── Heuristic 1: struct named *Registry ──────────────────────
            if let Some(name) = extract_struct_name(trimmed) {
                if name.ends_with("Registry") {
                    // Check current line AND up to 3 preceding lines for suppression.
                    let context_lines = gather_context_lines(ctx.source, line_num, 3);
                    emit_with_explanation(
                        &context_lines, line_num, ctx.crate_name,
                        crate::Severity::HARD_ERROR,
                        &format!("struct `{name}` looks like an ad-hoc registry — use `define_registry!` instead"),
                        &mut errors,
                    );
                }
            }

            // ── Heuristic 2: direct inventory crate usage ────────────────
            if trimmed.contains("inventory::submit!") || trimmed.contains("inventory::collect!") {
                let context_lines = gather_context_lines(ctx.source, line_num, 3);
                emit_with_explanation(
                    &context_lines, line_num, ctx.crate_name,
                    crate::Severity::HARD_ERROR,
                    "direct `inventory` usage outside macro definitions — use `define_registry!` which manages inventory integration",
                    &mut errors,
                );
            }

            // ── Heuristic 3: global mutable singleton (OnceLock<Mutex<) ──
            if trimmed.contains("OnceLock<Mutex<") || trimmed.contains("OnceLock<RwLock<") {
                let context_lines = gather_context_lines(ctx.source, line_num, 3);
                emit_with_explanation(
                    &context_lines, line_num, ctx.crate_name,
                    crate::Severity::ADVISORY,
                    "`OnceLock<Mutex/RwLock<...>>` — global mutable singletons often indicate an ad-hoc registry; prefer `define_registry!` + `define_resource!`",
                    &mut errors,
                );
            }

            // ── Heuristic 4: HashMap index-lookup (registry smell) ───────
            if trimmed.contains("HashMap<&str, usize>") || trimmed.contains("HashMap<String, usize>") {
                let context_lines = gather_context_lines(ctx.source, line_num, 3);
                emit_with_explanation(
                    &context_lines, line_num, ctx.crate_name,
                    crate::Severity::HARD_ERROR,
                    "`HashMap<&str/String, usize>` is a manual name→index registry — use `define_registry!` which provides `Registry::index_of()`",
                    &mut errors,
                );
            }
        }

        errors
    }
}

/// Gather the target line plus up to `look_back` preceding lines into one string
/// for suppression marker searching.
fn gather_context_lines(source: &str, target_line: usize, look_back: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = target_line.saturating_sub(look_back);
    lines[start..=target_line.min(lines.len() - 1)].join("\n")
}

/// Emit a lint violation respecting `// lint:allow(no_adhoc_framework) — <explanation>`.
///
/// Searches `context_lines` (the target line + preceding lines) for the marker.
///
/// - Explained suppression (8+ words after dash) → Warning
/// - Unexplained suppression → PushError
/// - No suppression → original severity
fn emit_with_explanation(
    context_lines: &str,
    line_num: usize,
    crate_name: &str,
    base_severity: crate::Severity,
    message: &str,
    errors: &mut Vec<LintError>,
) {
    if let Some(explanation) = extract_allow_explanation(context_lines) {
        if explanation.split_whitespace().count() >= 8 {
            errors.push(LintError::warning(
                crate_name.to_string(),
                line_num + 1,
                "no-adhoc-framework",
                format!("suppressed: {message} — {explanation}"),
            ));
        } else {
            errors.push(LintError::push_error(
                crate_name.to_string(),
                line_num + 1,
                "no-adhoc-framework",
                format!("suppressed without explanation (need 8+ words after `—`): {message}"),
            ));
        }
    } else {
        errors.push(LintError {
            crate_name: crate_name.to_string(),
            line: line_num + 1,
            lint_name: "no-adhoc-framework",
            severity: base_severity,
            message: message.to_string(),
            finding_kind: None,
        });
    }
}

/// Extract explanation text after `lint:allow(no_adhoc_framework) —`.
/// Searches across all lines in the context (target + preceding lines).
fn extract_allow_explanation(context: &str) -> Option<&str> {
    let marker = "lint:allow(no_adhoc_framework)";
    let pos = context.find(marker)?;
    let after = &context[pos + marker.len()..];

    // Take only up to the end of the line containing the marker.
    let rest = after.split('\n').next().unwrap_or(after);

    if let Some(dash_pos) = rest.find('—') {
        Some(rest[dash_pos + '—'.len_utf8()..].trim())
    } else if let Some(dash_pos) = rest.find(" - ") {
        Some(rest[dash_pos + 3..].trim())
    } else {
        Some("")
    }
}

/// Extract struct name from a line like `pub struct FooRegistry {` or `struct Bar;`.
fn extract_struct_name(line: &str) -> Option<&str> {
    let rest = if let Some(after) = line.strip_prefix("pub struct ") {
        after
    } else if let Some(after) = line.strip_prefix("struct ") {
        after
    } else {
        return None;
    };

    let name = rest
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}
