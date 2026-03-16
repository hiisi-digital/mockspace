//! Lint: no `define_raw_error!` outside crates that precede the diagnostics crate.
//!
//! `define_raw_error!` exists only for crates in the dep chain before
//! the diagnostics crate (i.e. `<prefix>-error-primitives`, `<prefix>-registry`).
//! Everything else must use `define_error!` from the diagnostics crate.

use crate::{Lint, LintContext, LintError};

pub struct NoRawErrorOutsidePrimitives;

impl Lint for NoRawErrorOutsidePrimitives {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        "no-raw-error-outside-primitives"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();

        for (line_num, line) in ctx.source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }

            if trimmed.contains("define_raw_error!") {
                if line.contains("lint:allow(no_raw_error)") {
                    errors.push(LintError::warning(
                        ctx.crate_name.to_string(),
                        line_num + 1,
                        "no-raw-error-outside-primitives",
                        "suppressed by lint:allow(no_raw_error) — review periodically".to_string(),
                    ));
                } else {
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_num + 1,
                        lint_name: "no-raw-error-outside-primitives",
                        severity: crate::Severity::HARD_ERROR,
                        message: "use `define_error!` from the diagnostics crate instead of `define_raw_error!`"
                            .to_string(),
                        finding_kind: None,
                    });
                }
            }
        }

        errors
    }
}
