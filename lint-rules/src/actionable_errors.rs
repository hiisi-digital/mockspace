//! Lint: every `define_error!` invocation must include a non-empty `hint:` field.
//!
//! Errors must be actionable — the user seeing the error should know what to do
//! next. The `hint:` field provides that guidance. An empty hint (`hint: ""`)
//! is treated the same as a missing hint.
//!
//! Also checks `define_warning!` and `define_hint!` invocations the same way.

use crate::{Lint, LintContext, LintError};

const TRACKED_MACROS: &[&str] = &["define_error!", "define_warning!", "define_hint!"];

pub struct ActionableErrors;

impl Lint for ActionableErrors {
    fn name(&self) -> &'static str {
        "actionable-errors"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let mut in_macro = false;
        let mut brace_depth: i32 = 0;
        let mut macro_start_line: usize = 0;
        let mut macro_name = "";
        let mut found_hint = false;

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Skip comments
            if trimmed.starts_with("//") {
                continue;
            }

            // Detect macro entry (must be an invocation, not an import/comment)
            if !in_macro {
                // Skip use/import lines
                if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
                    continue;
                }
                for &m in TRACKED_MACROS {
                    if trimmed.contains(m) {
                        in_macro = true;
                        brace_depth = 0;
                        macro_start_line = line_num + 1;
                        macro_name = m;
                        found_hint = false;
                        break;
                    }
                }
            }

            if in_macro {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;

                // Check for hint field presence
                if trimmed.starts_with("hint:") || trimmed.starts_with("hint :") {
                    // Check for empty hint: `hint: ""` or `hint: ""`
                    let after_colon = if let Some(rest) = trimmed.strip_prefix("hint:") {
                        rest
                    } else if let Some(rest) = trimmed.strip_prefix("hint :") {
                        rest
                    } else {
                        ""
                    };
                    let after_colon = after_colon.trim();

                    // Empty if the value is `""` or `''`
                    if after_colon == "\"\"," || after_colon == "\"\""
                        || after_colon == "''," || after_colon == "''"
                    {
                        // Empty hint — do not mark as found
                    } else {
                        found_hint = true;
                    }
                }

                // Detect macro end
                if brace_depth <= 0 && (trimmed.ends_with(");") || trimmed == ")") {
                    if !found_hint {
                        // Check the macro start line for lint:allow
                        let start_line = ctx.source.lines().nth(macro_start_line.saturating_sub(1)).unwrap_or("");
                        if start_line.contains("lint:allow(actionable_errors)") {
                            errors.push(LintError::warning(
                                ctx.crate_name.to_string(),
                                macro_start_line,
                                "actionable-errors",
                                format!("suppressed by lint:allow(actionable_errors) on {macro_name} — review periodically"),
                            ));
                        } else {
                            errors.push(LintError {
                                crate_name: ctx.crate_name.to_string(),
                                line: macro_start_line,
                                lint_name: "actionable-errors",
                                severity: crate::Severity::HARD_ERROR,
                                message: format!(
                                    "{macro_name} missing hint field — errors must be actionable \
                                     (include what to do next)",
                                ),
                                finding_kind: None,
                            });
                        }
                    }
                    in_macro = false;
                }
            }
        }

        errors
    }
}
