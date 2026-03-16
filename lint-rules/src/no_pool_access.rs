//! Lint: no deprecated behavior context API.
//!
//! Behaviors must declare resource access via `reads()` / `writes()` clauses
//! in `define_behavior!` and use `ctx.read::<T>()` / `ctx.write::<T>()`.
//!
//! Catches:
//! - `ctx.pool()` calls (removed from behavior context)
//! - `ctx.resource(` / `ctx.resource_mut(` (old names, now `read` / `write`)
//!
//! Note: `rt.pool()` and `rt.pool_mut()` in setup/main code are legitimate.
//! This lint uses line-level text matching inside `define_behavior!` macro
//! invocations, since tree-sitter sees macro bodies as token trees.

use crate::{Lint, LintContext, LintError};

pub struct NoPoolAccess;

impl Lint for NoPoolAccess {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        "no-pool-access"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();
        let mut in_behavior = false;
        let mut brace_depth: i32 = 0;

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();

            // Track define_behavior! macro boundaries
            if trimmed.starts_with("define_behavior!") {
                in_behavior = true;
                brace_depth = 0;
            }

            if in_behavior {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;

                // Check for banned patterns inside behavior body
                if trimmed.contains(".pool()") || trimmed.contains(".pool_mut()") {
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_num + 1,
                        lint_name: "no-pool-access",
                        severity: crate::Severity::HARD_ERROR,
                        message: "pool() is removed from behavior context; use ctx.read::<T>() or ctx.write::<T>() with reads()/writes() clauses".to_string(),
                        finding_kind: None,
                    });
                }
                if trimmed.contains(".resource(") || trimmed.contains(".resource::<") {
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_num + 1,
                        lint_name: "no-pool-access",
                        severity: crate::Severity::HARD_ERROR,
                        message: "resource() is renamed to read(); declare reads(T) in define_behavior!".to_string(),
                        finding_kind: None,
                    });
                }
                if trimmed.contains(".resource_mut(") || trimmed.contains(".resource_mut::<") {
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_num + 1,
                        lint_name: "no-pool-access",
                        severity: crate::Severity::HARD_ERROR,
                        message: "resource_mut() is renamed to write(); declare writes(T) in define_behavior!".to_string(),
                        finding_kind: None,
                    });
                }

                if brace_depth <= 0 && line.contains(')') {
                    in_behavior = false;
                }
            }
        }

        errors
    }
}
