//! Lint: no bare `Vec`, `HashMap`, or other stdlib/third-party collections.
//!
//! Framework code must use `Collection<T>`, `Dictionary<K, V>`, `DenseColumn<T>`,
//! `IdMap<K, V>`, `SparseArray<V>`, or `SparseSet<T>` from the storage crate.
//!
//! Two detection phases:
//! - Phase 1: AST-based detection in regular code (struct fields, fn signatures)
//! - Phase 2: text scanning inside `define_*!` macro bodies
//!
//! Severity: PushError (warning on commit, error on push with --strict).
//! Items marked with `// lint:allow(bare_collection)` comment are suppressed:
//! - With explanation (8+ words after the marker): Warning (never blocks).
//! - Without explanation: PushError (blocks push). You must explain WHY the
//!   bare collection is justified so other developers understand the exception.
//! Uses comment-based markers (not Rust attributes) to avoid rustc unknown-lint warnings.
//!
//! Subsumes the old `no-vec-in-macros` lint.

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "no-bare-vec";

/// (pattern to match as type_identifier, suggested replacement)
const FORBIDDEN_TYPES: &[(&str, &str)] = &[
    ("Vec", "Collection<T> or DenseColumn<T>"),
    ("HashMap", "Dictionary<K, V> or IdMap<K, V>"),
    ("HashSet", "SparseSet<T> or Collection<T>"),
    ("BTreeMap", "Dictionary<K, V> or IdMap<K, V>"),
    ("BTreeSet", "SparseSet<T> or Collection<T>"),
    ("VecDeque", "Collection<T>"),
    ("LinkedList", "Collection<T>"),
    ("BinaryHeap", "Collection<T>"),
    ("IndexMap", "Dictionary<K, V> or IdMap<K, V>"),
    ("IndexSet", "SparseSet<T> or Collection<T>"),
    ("SmallVec", "Collection<T>"),
    ("TinyVec", "Collection<T>"),
    ("ArrayVec", "Collection<T>"),
    ("SlotMap", "IdMap<K, V>"),
    ("DenseSlotMap", "IdMap<K, V>"),
    ("SecondaryMap", "IdMap<K, V>"),
];

/// Text patterns for Phase 2 macro body scanning (includes the `<` to reduce false positives).
const MACRO_FORBIDDEN: &[(&str, &str)] = &[
    ("Vec<", "Collection<T> or DenseColumn<T>"),
    ("HashMap<", "Dictionary<K, V> or IdMap<K, V>"),
    ("HashSet<", "SparseSet<T> or Collection<T>"),
    ("BTreeMap<", "Dictionary<K, V> or IdMap<K, V>"),
    ("BTreeSet<", "SparseSet<T> or Collection<T>"),
    ("VecDeque<", "Collection<T>"),
    ("LinkedList<", "Collection<T>"),
    ("BinaryHeap<", "Collection<T>"),
    ("IndexMap<", "Dictionary<K, V> or IdMap<K, V>"),
    ("IndexSet<", "SparseSet<T> or Collection<T>"),
    ("SmallVec<", "Collection<T>"),
    ("TinyVec<", "Collection<T>"),
    ("ArrayVec<", "Collection<T>"),
    ("SlotMap<", "IdMap<K, V>"),
    ("DenseSlotMap<", "IdMap<K, V>"),
    ("SecondaryMap<", "IdMap<K, V>"),
    // full paths
    ("std::collections::HashMap<", "Dictionary<K, V>"),
    ("std::collections::HashSet<", "SparseSet<T>"),
    ("std::collections::BTreeMap<", "Dictionary<K, V>"),
    ("std::collections::BTreeSet<", "SparseSet<T>"),
    ("std::collections::VecDeque<", "Collection<T>"),
    ("std::collections::LinkedList<", "Collection<T>"),
    ("std::collections::BinaryHeap<", "Collection<T>"),
];

pub struct NoBareVec;

impl Lint for NoBareVec {
    fn default_severity(&self) -> crate::Severity { crate::Severity::PUSH_GATE }
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
    // Skip macro definitions (they may use collection metavariables)
    if node.kind() == "macro_definition" {
        return;
    }

    // Skip extern "C" blocks (FFI boundary exemption)
    if node.kind() == "extern_block" {
        return;
    }

    // Skip #[cfg(test)] modules
    if node.kind() == "mod_item" && has_cfg_test_attr(node, ctx.source) {
        return;
    }

    // lint:allow(bare_collection) suppresses: with explanation → Warning, without → PushError
    if is_item_node(node) {
        if let Some(explanation) = has_allow_bare_collection(node, ctx.source) {
            let (severity, message) = if explanation_is_sufficient(&explanation) {
                (crate::Severity::ADVISORY,
                 format!("suppressed by lint:allow(bare_collection) — {explanation}"))
            } else {
                (crate::Severity::PUSH_GATE,
                 "lint:allow(bare_collection) requires an explanation (8+ words) — explain WHY this bare collection is justified".to_string())
            };
            errors.push(LintError {
                crate_name: ctx.crate_name.to_string(),
                line: node.start_position().row + 1,
                lint_name: LINT_NAME,
                severity,
                message,
            });
            return;
        }
    }

    // Check type_identifier nodes for forbidden collection names
    if node.kind() == "type_identifier" {
        let text = txt(node, ctx.source);
        for &(forbidden, replacement) in FORBIDDEN_TYPES {
            if text == forbidden && is_type_position(node) {
                // Check same-line lint:allow (catches trailing comments like `field: Vec<T>, // lint:allow(...)`)
                let line_idx = node.start_position().row;
                let source_line = ctx.source.lines().nth(line_idx).unwrap_or("");
                if let Some(explanation) = line_allow_explanation(source_line) {
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (crate::Severity::ADVISORY,
                         format!("suppressed by lint:allow(bare_collection) — {explanation}"))
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) requires an explanation (8+ words) — explain WHY this bare collection is justified".to_string())
                    };
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: line_idx + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                    });
                    break;
                }
                // Check preceding line of the field/variant for lint:allow
                if let Some(explanation) = field_level_allow_explanation(node, ctx.source) {
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (crate::Severity::ADVISORY,
                         format!("suppressed by lint:allow(bare_collection) — {explanation}"))
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) requires an explanation (8+ words) — explain WHY this bare collection is justified".to_string())
                    };
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: node.start_position().row + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                    });
                    break;
                }
                // lint:allow(bare_collection) on enclosing item: explained → Warning, unexplained → PushError
                if let Some(explanation) = enclosing_item_explanation(node, ctx.source) {
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (crate::Severity::ADVISORY,
                         format!("suppressed by lint:allow(bare_collection) on enclosing item — {explanation}"))
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) on enclosing item requires an explanation (8+ words)".to_string())
                    };
                    errors.push(LintError {
                        crate_name: ctx.crate_name.to_string(),
                        line: node.start_position().row + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                    });
                    break;
                }
                let severity = crate::Severity::PUSH_GATE;
                errors.push(LintError {
                    crate_name: ctx.crate_name.to_string(),
                    line: node.start_position().row + 1,
                    lint_name: LINT_NAME,
                    severity,
                    message: format!(
                        "`{forbidden}` collection type — use `{replacement}` from the storage crate instead",
                    ),
                });
                break;
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_nodes(child, ctx, errors);
    }
}

/// Returns true if the node is in a type-annotation position (struct field,
/// fn parameter, return type, let binding, const/static, type alias, generic arg).
fn is_type_position(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            // Intermediate type nodes — keep walking up
            "generic_type" | "reference_type" | "array_type" | "tuple_type"
            | "pointer_type" | "scoped_type_identifier" | "type_arguments" => {}
            // Definite type-annotation parents
            "function_item" | "function_signature_item" | "parameter"
            | "field_declaration" | "let_declaration" | "const_item"
            | "static_item" | "type_item" | "return_type" | "closure_parameters"
            | "impl_item" | "trait_item" | "where_predicate" | "type_bound" => {
                return true;
            }
            // Expression position — not a type annotation (e.g., Vec::new())
            "call_expression" | "field_expression" | "scoped_identifier"
            | "macro_invocation" => {
                return false;
            }
            _ => {
                // For other parent kinds, keep walking up
            }
        }
        current = parent.parent();
    }
    false
}

/// Check whether a node is a top-level item (struct, fn, enum, impl, etc.).
fn is_item_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "function_item" | "function_signature_item" | "struct_item"
        | "enum_item" | "impl_item" | "const_item" | "static_item"
    )
}

/// Walk up from a type_identifier through its containing field_declaration,
/// enum_variant, let_declaration, etc. and check each level for a
/// preceding-sibling lint:allow comment. Does NOT stop at the first container;
/// continues up so that a comment on an enum_variant covers fields inside it.
fn field_level_allow_explanation(node: Node, source: &str) -> Option<String> {
    let target = "lint:allow(bare_collection)";
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "field_declaration" | "enum_variant" | "let_declaration" | "const_item" | "static_item" => {
                if let Some(explanation) = has_allow_bare_collection(parent, source) {
                    return Some(explanation);
                }
                // not found here; keep walking up (e.g. field inside enum variant)
            }
            // Keep walking up through intermediate type/container nodes
            "generic_type" | "reference_type" | "array_type" | "tuple_type"
            | "pointer_type" | "scoped_type_identifier" | "type_arguments"
            | "field_declaration_list" | "declaration_list" | "enum_variant_list" => {}
            _ => {
                // Last resort: check the line before this parent for a comment
                let start_row = parent.start_position().row;
                if start_row > 0 {
                    if let Some(prev_line) = source.lines().nth(start_row - 1) {
                        if prev_line.contains(target) {
                            return Some(extract_explanation(prev_line, target));
                        }
                    }
                }
                break;
            }
        }
        current = parent.parent();
    }
    None
}

/// Walk up from a node to the enclosing item and return the explanation if lint:allow found.
fn enclosing_item_explanation(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if is_item_node(parent) {
            if let Some(explanation) = has_allow_bare_collection(parent, source) {
                return Some(explanation);
            }
        }
        current = parent.parent();
    }
    None
}

/// Minimum word count for a lint:allow explanation to be considered sufficient.
const MIN_EXPLANATION_WORDS: usize = 8;

/// Check whether an item has a `// lint:allow(bare_collection)` comment marker.
/// Returns `Some(explanation)` if found (explanation may be empty), `None` if not found.
/// Uses comment-based markers (not Rust attributes) to avoid rustc unknown-lint warnings.
/// Checks preceding sibling comments and inline comments within the item.
fn has_allow_bare_collection(node: Node, source: &str) -> Option<String> {
    let target = "lint:allow(bare_collection)";

    // Check preceding sibling comments
    if let Some(parent) = node.parent() {
        let mut sibling_cursor = parent.walk();
        let mut found_explanation: Option<String> = None;
        for child in parent.children(&mut sibling_cursor) {
            if child.id() == node.id() {
                break;
            }
            if child.kind() == "line_comment" || child.kind() == "block_comment" {
                let comment_text = txt(child, source);
                if comment_text.contains(target) {
                    found_explanation = Some(extract_explanation(comment_text, target));
                }
            } else if child.kind() != "attribute_item" {
                found_explanation = None;
            }
        }
        if found_explanation.is_some() {
            return found_explanation;
        }
    }

    // Check child comments (e.g. inside a struct body)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "line_comment" || child.kind() == "block_comment" {
            let comment_text = txt(child, source);
            if comment_text.contains(target) {
                return Some(extract_explanation(comment_text, target));
            }
        }
    }

    None
}

/// Extract explanation text after a `lint:allow(bare_collection)` marker.
/// Accepts separators: ` — `, ` - `, `: `, or just whitespace after the closing `)`.
fn extract_explanation(comment: &str, target: &str) -> String {
    if let Some(pos) = comment.find(target) {
        let after = &comment[pos + target.len()..];
        let trimmed = after.trim();
        // Strip leading separator: " — ", " - ", ": "
        let explanation = if trimmed.starts_with("—") || trimmed.starts_with("–") {
            trimmed[trimmed.char_indices().nth(1).map(|(i, _)| i).unwrap_or(1)..].trim()
        } else if trimmed.starts_with('-') {
            trimmed[1..].trim()
        } else if trimmed.starts_with(':') {
            trimmed[1..].trim()
        } else {
            trimmed
        };
        explanation.to_string()
    } else {
        String::new()
    }
}

/// Check if an explanation has enough words to be considered sufficient.
fn explanation_is_sufficient(explanation: &str) -> bool {
    explanation.split_whitespace().count() >= MIN_EXPLANATION_WORDS
}

/// Check if a source line has a lint:allow(bare_collection) comment (trailing or otherwise).
/// Returns Some(explanation) if found, None otherwise.
fn line_allow_explanation(line: &str) -> Option<String> {
    let target = "lint:allow(bare_collection)";
    if line.contains(target) {
        Some(extract_explanation(line, target))
    } else {
        None
    }
}

/// Check whether a mod_item has a `#[cfg(test)]` attribute.
/// Checks both children and preceding siblings (tree-sitter-rust puts attributes
/// as preceding sibling nodes).
fn has_cfg_test_attr(node: Node, source: &str) -> bool {
    // Check children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "attribute" {
            let attr_text = txt(child, source);
            if attr_text.contains("cfg") && attr_text.contains("test") {
                return true;
            }
        }
    }

    // Check preceding siblings
    if let Some(parent) = node.parent() {
        let mut sibling_cursor = parent.walk();
        let mut found_cfg_test = false;
        for child in parent.children(&mut sibling_cursor) {
            if child.id() == node.id() {
                break;
            }
            if child.kind() == "attribute_item" {
                let attr_text = txt(child, source);
                if attr_text.contains("cfg") && attr_text.contains("test") {
                    found_cfg_test = true;
                } else {
                    found_cfg_test = false;
                }
            } else if child.kind() != "line_comment" && child.kind() != "block_comment" {
                found_cfg_test = false;
            }
        }
        if found_cfg_test {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Phase 2: text scanning inside define_*! macro invocations
// ---------------------------------------------------------------------------

fn scan_macro_bodies(ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut in_macro = false;
    let mut brace_depth: i32 = 0;
    let mut in_cfg_test = false;
    let mut cfg_test_depth: i32 = 0;
    let mut prev_line_allow = false;
    let mut prev_line_explanation = String::new();
    let allow_target = "lint:allow(bare_collection)";

    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments but track lint:allow on comment-only lines
        if trimmed.starts_with("//") {
            if trimmed.contains(allow_target) {
                prev_line_allow = true;
                prev_line_explanation = extract_explanation(trimmed, allow_target);
            }
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

            // Skip string literals
            if trimmed.starts_with('"') || trimmed.starts_with("r#\"") {
                // fall through to brace-depth tracking
            } else {
                // Check for same-line or preceding-line lint:allow
                let has_allow = line.contains(allow_target) || prev_line_allow;
                if has_allow {
                    let explanation = if line.contains(allow_target) {
                        extract_explanation(line, allow_target)
                    } else {
                        prev_line_explanation.clone()
                    };
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (crate::Severity::ADVISORY,
                         format!("suppressed by lint:allow(bare_collection) in macro — {explanation}"))
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) in macro requires an explanation (8+ words)".to_string())
                    };
                    // Still emit a diagnostic so it's visible
                    for &(pattern, _) in MACRO_FORBIDDEN {
                        if trimmed.contains(pattern) {
                            errors.push(LintError {
                                crate_name: ctx.crate_name.to_string(),
                                line: line_num + 1,
                                lint_name: LINT_NAME,
                                severity,
                                message: message.clone(),
                            });
                            break;
                        }
                    }
                } else {
                    for &(pattern, replacement) in MACRO_FORBIDDEN {
                        if trimmed.contains(pattern) {
                            let col_name = pattern.split('<').next().unwrap_or(pattern);
                            errors.push(LintError {
                                crate_name: ctx.crate_name.to_string(),
                                line: line_num + 1,
                                lint_name: LINT_NAME,
                                severity: crate::Severity::PUSH_GATE,
                                message: format!(
                                    "`{col_name}` in macro body — use `{replacement}` from the storage crate instead",
                                ),
                            });
                            // one error per line
                            break;
                        }
                    }
                }
            }

            if brace_depth <= 0 && (trimmed.ends_with(");") || trimmed == ")") {
                in_macro = false;
            }
        }

        // Reset prev_line_allow after processing a non-comment line
        prev_line_allow = false;
        prev_line_explanation.clear();
    }
}

fn txt<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}
