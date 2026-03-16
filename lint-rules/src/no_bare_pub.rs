//! Lint: no bare `pub` without `#[public_api]` or `#[internal_api]`.
//!
//! Every `pub` item in a workspace crate must be annotated with either
//! `#[public_api]` or `#[internal_api]`. Scoped visibility (`pub(crate)`,
//! `pub(super)`) is allowed without annotation.
//!
//! This ensures every public item has explicit intent about its API surface,
//! enabling the visibility macros to control `#[doc(hidden)]` and `pub(crate)`
//! based on the `standalone` feature flag.
//!
//! Exempt: items inside macro definitions, test modules, and the visibility
//! macros crate itself.

use crate::{Lint, LintContext, LintError};

pub struct NoBarePublic;

impl Lint for NoBarePublic {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        "no-bare-pub"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let mut in_macro_def = false;
        let mut brace_depth_macro: i32 = 0;
        let mut module_depth: i32 = 0;

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            // Track macro definitions
            if trimmed.contains("macro_rules!") {
                in_macro_def = true;
                brace_depth_macro = 0;
            }

            if in_macro_def {
                brace_depth_macro += line.matches('{').count() as i32;
                brace_depth_macro -= line.matches('}').count() as i32;
                if brace_depth_macro <= 0 && line.contains('}') {
                    in_macro_def = false;
                }
                continue;
            }

            // Only check items at module level (depth 0).
            // Items inside impl blocks, inner mods, cfg blocks, etc. are deeper.
            if module_depth == 0 && is_bare_pub(trimmed) {
                let has_api_attr = has_visibility_attribute(ctx.source, line_num);
                if !has_api_attr {
                    let item_preview: String = trimmed.chars().take(60).collect();
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_num + 1,
                        lint_name: "no-bare-pub",
                        severity: crate::Severity::HARD_ERROR,
                        message: format!(
                            "bare `pub` without `#[public_api]` or `#[internal_api]`: {item_preview}..."
                        ),
                        finding_kind: None,
                    });
                }
            }

            // Update brace depth after checking (so the opening line of a
            // struct/impl/mod is checked at its parent depth)
            module_depth += line.matches('{').count() as i32;
            module_depth -= line.matches('}').count() as i32;
            if module_depth < 0 {
                module_depth = 0;
            }
        }

        errors
    }
}

/// Check if a line starts with bare `pub` (not scoped like `pub(crate)`).
fn is_bare_pub(trimmed: &str) -> bool {
    // Match lines starting with `pub ` but not `pub(` or `pub use` of macros
    if !trimmed.starts_with("pub ") && !trimmed.starts_with("pub(") {
        return false;
    }

    // pub(crate), pub(super), pub(in ...) are scoped — allowed
    if trimmed.starts_with("pub(crate)") || trimmed.starts_with("pub(super)") || trimmed.starts_with("pub(in ") {
        return false;
    }

    // pub( with anything else is scoped
    if trimmed.starts_with("pub(") {
        return false;
    }

    // Skip `pub use` re-exports (re-exports are fine without annotation)
    if trimmed.starts_with("pub use ") {
        return false;
    }

    // Skip `pub mod` — proc macro attrs on file modules are unstable
    if trimmed.starts_with("pub mod ") {
        return false;
    }

    true
}

/// Look backwards from the given line for `#[public_api]` or `#[internal_api]`.
fn has_visibility_attribute(source: &str, target_line: usize) -> bool {
    let lines: Vec<&str> = source.lines().collect();

    // Look at the preceding lines (up to 5 lines back for multi-line attributes)
    for i in (0..target_line).rev().take(5) {
        let prev = lines[i].trim();
        if prev.is_empty() || prev.starts_with("//") {
            continue;
        }
        if prev.contains("#[public_api]") || prev.contains("#[internal_api]") {
            return true;
        }
        // If we hit a non-attribute, non-comment, non-empty line, stop
        if !prev.starts_with("#[") && !prev.starts_with("///") {
            break;
        }
    }

    false
}
