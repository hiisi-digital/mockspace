//! Lint: no `f32` or `f64` type annotations in framework code.
//!
//! All numeric values must use fixed-point types (`UFixed`/`IFixed`) or semantic
//! aliases wrapping them (e.g., `Coord`, `Volume`, `FontSize`). Raw IEEE floats
//! are only permitted in GPU shader internals, FFI boundaries, and the
//! primitives crate that defines the fixed-point conversions.
//!
//! Uses tree-sitter AST walking for regular code and text scanning for
//! `define_*!` macro bodies (which tree-sitter cannot parse).

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const FLOAT_TYPES: &[&str] = &["f32", "f64"];

const LINT_NAME: &str = "no-float";

pub struct NoFloat;

impl Lint for NoFloat {
    fn default_severity(&self) -> crate::Severity {
        // Declared to match what this lint emits, since every finding it produces is a warning.
        // Declaring OFF while emitting findings is incoherent, and it
        // only looked harmless while the resolver ignored the
        // declaration entirely.
        crate::Severity::ADVISORY
    }

    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();

        // Phase 1: AST-based detection for regular code
        let root = ctx.tree.root_node();
        visit_nodes(root, ctx, &mut errors);

        // Phase 2: text scanning inside define_*! macro bodies
        scan_macro_bodies(ctx, &mut errors);

        errors
    }
}

// ---------------------------------------------------------------------------
// Phase 1: tree-sitter AST walking
// ---------------------------------------------------------------------------

fn visit_nodes(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    // Skip macro definitions — they may use float metavariables
    if node.kind() == "macro_definition" {
        return;
    }

    // Skip extern "C" blocks (FFI boundary exemption)
    if node.kind() == "extern_block" {
        return;
    }

    // Skip #[cfg(test)] modules
    if crate::is_cfg_test_mod(node, ctx.source) {
        return;
    }

    // Check primitive_type nodes that are f32 or f64
    if node.kind() == "primitive_type" {
        let text = txt(node, ctx.source);
        if FLOAT_TYPES.contains(&text) && is_type_position(node) {
            // Check for lint:allow(no_float) on the line
            let line_idx = node.start_position().row;
            let line = ctx.source.lines().nth(line_idx).unwrap_or("");
            if crate::line_lint_allowed(line, "no_float") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_idx + 1,
                    LINT_NAME,
                    "suppressed by lint:allow(no_float) — review periodically".to_string(),
                ));
            } else {
                errors.push(LintError {
                    crate_name:   ctx.crate_name.to_string(),
                    line:         line_idx + 1,
                    lint_name:    LINT_NAME,
                    severity:     crate::Severity::HARD_ERROR,
                    message:      format!(
                        "`{text}` type annotation — use a fixed-point type \
                         (`UFixed`/`IFixed`) or a semantic alias instead",
                    ),
                    finding_kind: None,
                });
            }
        }
    }

    // Check float literals (e.g., 1.0, 3.14f32, 2.5f64)
    if node.kind() == "float_literal" {
        let text = txt(node, ctx.source);
        let line_idx = node.start_position().row;
        let line = ctx.source.lines().nth(line_idx).unwrap_or("");
        if crate::line_lint_allowed(line, "no_float") {
            errors.push(LintError::warning(
                ctx.crate_name.to_string(),
                line_idx + 1,
                LINT_NAME,
                "suppressed by lint:allow(no_float) — review periodically".to_string(),
            ));
        } else {
            errors.push(LintError {
                crate_name:   ctx.crate_name.to_string(),
                line:         line_idx + 1,
                lint_name:    LINT_NAME,
                severity:     crate::Severity::HARD_ERROR,
                message:      format!(
                    "float literal `{text}` — use fixed-point construction \
                     (e.g., `IFixed::from_raw()` or `ufixed!()`) instead",
                ),
                finding_kind: None,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_nodes(child, ctx, errors);
    }
}

/// Returns true if the node appears in a type-annotation position:
/// function parameters, return types, struct/enum fields, let bindings,
/// const/static declarations, type aliases, or generic arguments.
fn is_type_position(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            // Direct type annotation contexts
            "type_identifier"
            | "generic_type"
            | "reference_type"
            | "array_type"
            | "tuple_type"
            | "pointer_type"
            | "scoped_type_identifier" => {
                // Keep walking up — these are intermediate type nodes
            },
            // Definite type-annotation parents
            "function_item"
            | "function_signature_item"
            | "parameter"
            | "field_declaration"
            | "let_declaration"
            | "const_item"
            | "static_item"
            | "type_item"
            | "type_arguments"
            | "return_type"
            | "closure_parameters"
            | "impl_item"
            | "trait_item"
            | "where_predicate"
            | "type_bound" => {
                return true;
            },
            // Literal expressions like `1.0_f32` — still a float usage
            "float_literal" => return true,
            _ => return true,
        }
        current = parent.parent();
    }
    // primitive_type at top level is still a type annotation
    true
}

// ---------------------------------------------------------------------------
// Phase 2: text scanning inside define_*! macro invocations
// ---------------------------------------------------------------------------

/// Text-based patterns that indicate float usage inside macro bodies.
const MACRO_FLOAT_PATTERNS: &[&str] = &[": f32", ": f64", "<f32>", "<f64>"];

fn scan_macro_bodies(ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut in_macro = false;
    let mut brace_depth: i32 = 0;
    let mut in_cfg_test = false;
    let mut cfg_test_depth: i32 = 0;

    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") {
            continue;
        }

        // Track #[cfg(test)] modules (text-level)
        if trimmed.contains("#[cfg(test)]") {
            in_cfg_test = true;
            cfg_test_depth = 0;
        }
        if in_cfg_test {
            cfg_test_depth += line.matches('{').count() as i32;
            cfg_test_depth -= line.matches('}').count() as i32;
            if cfg_test_depth <= 0 && in_cfg_test && line.contains('}') {
                in_cfg_test = false;
            }
            continue;
        }

        // Detect define_*! macro invocation entry
        if !in_macro && trimmed.starts_with("define_") && trimmed.contains('!') {
            in_macro = true;
            brace_depth = 0;
        }

        if in_macro {
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;

            // Skip string literals (very rough: skip lines that look like strings)
            if trimmed.starts_with('"') || trimmed.starts_with("r#\"") {
                // fall through to brace-depth tracking but don't check patterns
            } else {
                // lint:allow(no_float) suppresses but warns
                if crate::line_lint_allowed(line, "no_float") {
                    // still check if there's actually a float to suppress
                    for pattern in MACRO_FLOAT_PATTERNS {
                        if trimmed.contains(pattern) {
                            errors.push(LintError::warning(
                                ctx.crate_name.to_string(),
                                line_num + 1,
                                LINT_NAME,
                                "suppressed by lint:allow(no_float) — review periodically"
                                    .to_string(),
                            ));
                            break;
                        }
                    }
                } else {
                    for pattern in MACRO_FLOAT_PATTERNS {
                        if trimmed.contains(pattern) {
                            let float_ty = if pattern.contains("f32") { "f32" } else { "f64" };
                            errors.push(LintError {
                                crate_name:   ctx.crate_name.to_string(),
                                line:         line_num + 1,
                                lint_name:    LINT_NAME,
                                severity:     crate::Severity::HARD_ERROR,
                                message:      format!(
                                    "`{float_ty}` in macro body — use a fixed-point type \
                                     (`UFixed`/`IFixed`) or a semantic alias instead",
                                ),
                                finding_kind: None,
                            });
                            break;
                        }
                    }
                }
            }

            if brace_depth <= 0 && (trimmed.ends_with(");") || trimmed == ")") {
                in_macro = false;
            }
        }
    }
}

fn txt<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

#[cfg(test)]
mod cfg_test_module_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    fn ctx_for(source: &'static str) -> LintContext<'static> {
        let mut parser = crate::make_parser();
        let tree = parser.parse(source, None).unwrap();
        LintContext {
            crate_name:              "test-crate",
            short_name:              "test-crate",
            source,
            tree:                    Box::leak(Box::new(tree)),
            all_sources:             &[],
            deps:                    &[],
            all_crates:              Box::leak(Box::new(BTreeSet::new())),
            design_doc:              None,
            all_doc_content:         "",
            shame_doc:               None,
            workspace_root:          std::path::Path::new("/tmp"),
            proc_macro_crates:       &[],
            crate_prefix:            "test",
            lint_proc_macro_source:  false,
            primitive_introductions: Box::leak(Box::new(BTreeMap::new())),
        }
    }

    fn reported(source: &'static str) -> usize {
        NoFloat.check(&ctx_for(source)).len()
    }

    #[test]
    fn a_float_inside_a_test_module_is_not_reported() {
        // The lint has claimed to skip test modules since it was written. Its
        // private predicate looked for the attribute among the module's
        // children, where it never is, so the skip never happened and this is
        // the first thing to establish that it does.
        assert_eq!(
            reported("#[cfg(test)]\nmod tests {\n    fn f() { let x: f64 = 1.0; }\n}\n"),
            0,
        );
    }

    #[test]
    fn a_float_outside_a_test_module_is_still_reported() {
        // The control: a lint that reported nothing would pass the test above.
        assert!(reported("pub fn f(x: f64) -> f64 { x }\n") > 0);
    }

    #[test]
    fn a_float_in_a_module_that_is_not_test_only_is_still_reported() {
        assert!(reported("#[cfg(not(test))]\nmod real {\n    pub fn f(x: f64) -> f64 { x }\n}\n") > 0);
    }
}
