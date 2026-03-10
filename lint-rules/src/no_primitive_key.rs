//! Lint: no primitive-keyed maps.
//!
//! `HashMap`, `BTreeMap`, etc. should not be keyed on primitive types
//! (`String`, `&str`, `u8`..`u128`, `usize`, `i8`..`i128`, `isize`).
//! Use `define_id!` types instead.
//!
//! Exempt: the ID infrastructure crate (it IS the ID infrastructure).

use crate::{Lint, LintContext, LintError};

const MAP_TYPES: &[&str] = &["HashMap", "BTreeMap"];

const PRIMITIVE_KEY_TYPES: &[&str] = &[
    "String", "&str",
    "u8", "u16", "u32", "u64", "u128", "usize",
    "i8", "i16", "i32", "i64", "i128", "isize",
];

pub struct NoPrimitiveKey;

impl Lint for NoPrimitiveKey {
    fn name(&self) -> &'static str {
        "no-primitive-key"
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
            if is_in_macro_def_line(ctx.source, line_num) {
                continue;
            }

            for map_type in MAP_TYPES {
                if let Some(pos) = trimmed.find(&format!("{map_type}<")) {
                    let after_angle = &trimmed[pos + map_type.len() + 1..];
                    let key_type = after_angle
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim();

                    for prim in PRIMITIVE_KEY_TYPES {
                        if key_type == *prim {
                            if line.contains("lint:allow(no_primitive_key)") {
                                errors.push(LintError::warning(
                                    ctx.crate_name.to_string(),
                                    line_num + 1,
                                    "no-primitive-key",
                                    "suppressed by lint:allow(no_primitive_key) — review periodically".to_string(),
                                ));
                            } else {
                                errors.push(LintError {
                                    crate_name: ctx.crate_name.to_string(),
                                    line: line_num + 1,
                                    lint_name: "no-primitive-key",
                                    severity: crate::Severity::HARD_ERROR,
                                    message: format!(
                                        "`{map_type}<{prim}, ...>` — use a define_id! type as key",
                                    ),
                                });
                            }
                            break;
                        }
                    }
                }
            }
        }

        errors
    }
}

fn is_in_macro_def_line(source: &str, target_line: usize) -> bool {
    let mut depth: i32 = 0;
    let mut in_macro = false;

    for (i, line) in source.lines().enumerate() {
        if i >= target_line && in_macro && depth > 0 {
            return true;
        }

        let trimmed = line.trim();
        if trimmed.contains("macro_rules!") {
            in_macro = true;
            depth = 0;
        }

        if in_macro {
            depth += trimmed.chars().filter(|c| *c == '{').count() as i32;
            depth -= trimmed.chars().filter(|c| *c == '}').count() as i32;
            if depth <= 0 && i > 0 {
                in_macro = false;
            }
        }

        if i >= target_line {
            break;
        }
    }

    false
}
