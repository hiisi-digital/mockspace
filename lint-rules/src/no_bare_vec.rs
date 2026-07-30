//! Lint: no bare `Vec`, `HashMap`, or other stdlib/third-party collections.
//!
//! A container named in a signature decides three things on the caller's behalf:
//! which container, which layout, and where the memory comes from. None of those
//! are the callee's to pick. The fix is never a different concrete container; it
//! is to name the contract the position actually needs as a trait, take it as a
//! bound, and let whoever calls it supply the container and its allocation.
//!
//! So the guidance below describes contract shapes rather than types. An earlier
//! version named six concrete types from a downstream consumer's storage crate,
//! none of which exist in any repository this lint governs, so every consumer was
//! being pointed at types it could not use and at a decision that was not the
//! lint's to make.
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

/// What a position needs, once the container is taken out of it.
///
/// One string per shape rather than per type, because every map wants the same
/// advice and repeating it per name is how the advice drifts.
const SEQUENCE: &str =
    "This one is a sequence: take `impl IntoIterator<Item = T>` or `&[T]` to read one, and a \
     push-shaped bound to fill one.";
const KEYED: &str = "This one is a keyed lookup: take a bound carrying the get and insert this \
                     position actually uses, and nothing more.";
const MEMBERSHIP: &str = "This one is membership: take a bound carrying the contains and insert \
                          this position actually uses, and nothing more.";

/// (pattern to match as type_identifier, the contract shape the position needs)
const FORBIDDEN_TYPES: &[(&str, &str)] = &[
    ("Vec", SEQUENCE),
    ("HashMap", KEYED),
    ("HashSet", MEMBERSHIP),
    ("BTreeMap", KEYED),
    ("BTreeSet", MEMBERSHIP),
    ("VecDeque", SEQUENCE),
    ("LinkedList", SEQUENCE),
    ("BinaryHeap", SEQUENCE),
    ("IndexMap", KEYED),
    ("IndexSet", MEMBERSHIP),
    ("SmallVec", SEQUENCE),
    ("TinyVec", SEQUENCE),
    ("ArrayVec", SEQUENCE),
    ("SlotMap", KEYED),
    ("DenseSlotMap", KEYED),
    ("SecondaryMap", KEYED),
];

/// Text patterns for Phase 2 macro body scanning (includes the `<` to reduce false positives).
const MACRO_FORBIDDEN: &[(&str, &str)] = &[
    ("Vec<", SEQUENCE),
    ("HashMap<", KEYED),
    ("HashSet<", MEMBERSHIP),
    ("BTreeMap<", KEYED),
    ("BTreeSet<", MEMBERSHIP),
    ("VecDeque<", SEQUENCE),
    ("LinkedList<", SEQUENCE),
    ("BinaryHeap<", SEQUENCE),
    ("IndexMap<", KEYED),
    ("IndexSet<", MEMBERSHIP),
    ("SmallVec<", SEQUENCE),
    ("TinyVec<", SEQUENCE),
    ("ArrayVec<", SEQUENCE),
    ("SlotMap<", KEYED),
    ("DenseSlotMap<", KEYED),
    ("SecondaryMap<", KEYED),
    // full paths
    ("std::collections::HashMap<", KEYED),
    ("std::collections::HashSet<", MEMBERSHIP),
    ("std::collections::BTreeMap<", KEYED),
    ("std::collections::BTreeSet<", MEMBERSHIP),
    ("std::collections::VecDeque<", SEQUENCE),
    ("std::collections::LinkedList<", SEQUENCE),
    ("std::collections::BinaryHeap<", SEQUENCE),
];

pub struct NoBareVec;

impl Lint for NoBareVec {
    fn default_severity(&self) -> crate::Severity {
        // Declared to match what this lint emits, since it produces push-gate findings.
        // Declaring OFF while emitting findings is incoherent, and it
        // only looked harmless while the resolver ignored the
        // declaration entirely.
        crate::Severity::PUSH_GATE
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
    // Skip macro definitions (they may use collection metavariables)
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

    // lint:allow(bare_collection) suppresses: with explanation → Warning, without → PushError
    if is_item_node(node) {
        if let Some(explanation) = has_allow_bare_collection(node, ctx.source) {
            let (severity, message) = if explanation_is_sufficient(&explanation) {
                (
                    crate::Severity::ADVISORY,
                    format!("suppressed by lint:allow(bare_collection): {explanation}"),
                )
            } else {
                (crate::Severity::PUSH_GATE,
                 "lint:allow(bare_collection) requires an explanation (8+ words): say why this bare collection is justified".to_string())
            };
            errors.push(LintError {
                path: None,
                crate_name: ctx.crate_name.to_string(),
                line: node.start_position().row + 1,
                lint_name: LINT_NAME,
                severity,
                message,
                finding_kind: None,
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
                        (
                            crate::Severity::ADVISORY,
                            format!("suppressed by lint:allow(bare_collection): {explanation}"),
                        )
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) requires an explanation (8+ words): say why this bare collection is justified".to_string())
                    };
                    errors.push(LintError {
                        path: None,
                        crate_name: ctx.crate_name.to_string(),
                        line: line_idx + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                        finding_kind: None,
                    });
                    break;
                }
                // Check preceding line of the field/variant for lint:allow
                if let Some(explanation) = field_level_allow_explanation(node, ctx.source) {
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (
                            crate::Severity::ADVISORY,
                            format!("suppressed by lint:allow(bare_collection): {explanation}"),
                        )
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) requires an explanation (8+ words): say why this bare collection is justified".to_string())
                    };
                    errors.push(LintError {
                        path: None,
                        crate_name: ctx.crate_name.to_string(),
                        line: node.start_position().row + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                        finding_kind: None,
                    });
                    break;
                }
                // lint:allow(bare_collection) on enclosing item: explained → Warning, unexplained → PushError
                if let Some(explanation) = enclosing_item_explanation(node, ctx.source) {
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (
                            crate::Severity::ADVISORY,
                            format!(
                                "suppressed by lint:allow(bare_collection) on enclosing item: {explanation}"
                            ),
                        )
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) on enclosing item requires an explanation (8+ words)".to_string())
                    };
                    errors.push(LintError {
                        path: None,
                        crate_name: ctx.crate_name.to_string(),
                        line: node.start_position().row + 1,
                        lint_name: LINT_NAME,
                        severity,
                        message,
                        finding_kind: None,
                    });
                    break;
                }
                let severity = crate::Severity::PUSH_GATE;
                errors.push(LintError {
                    path: None,
                    crate_name: ctx.crate_name.to_string(),
                    line: node.start_position().row + 1,
                    lint_name: LINT_NAME,
                    severity,
                    message: format!(
                        "`{forbidden}` names a container here. Name the contract instead: declare what this \
                          position needs as a trait, take it as a bound, and let the caller supply \
                          the container and its allocation. {replacement}",
                    ),
                    finding_kind: None,
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
            // Intermediate type nodes: keep walking up
            "generic_type"
            | "reference_type"
            | "array_type"
            | "tuple_type"
            | "pointer_type"
            | "scoped_type_identifier"
            | "type_arguments" => {},
            // Definite type-annotation parents
            "function_item"
            | "function_signature_item"
            | "parameter"
            | "field_declaration"
            | "let_declaration"
            | "const_item"
            | "static_item"
            | "type_item"
            | "return_type"
            | "closure_parameters"
            | "impl_item"
            | "trait_item"
            | "where_predicate"
            | "type_bound" => {
                return true;
            },
            // Expression position, not a type annotation (e.g. Vec::new())
            "call_expression" | "field_expression" | "scoped_identifier" | "macro_invocation" => {
                return false;
            },
            _ => {
                // For other parent kinds, keep walking up
            },
        }
        current = parent.parent();
    }
    false
}

/// Check whether a node is a top-level item (struct, fn, enum, impl, etc.).
fn is_item_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "impl_item"
            | "const_item"
            | "static_item"
    )
}

/// Walk up from a type_identifier through its containing field_declaration,
/// enum_variant, let_declaration, etc. and check each level for a
/// preceding-sibling lint:allow comment. Does NOT stop at the first container;
/// continues up so that a comment on an enum_variant covers fields inside it.
fn field_level_allow_explanation(node: Node, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "field_declaration" | "enum_variant" | "let_declaration" | "const_item"
            | "static_item" => {
                if let Some(explanation) = has_allow_bare_collection(parent, source) {
                    return Some(explanation);
                }
                // not found here; keep walking up (e.g. field inside enum variant)
            },
            // Keep walking up through intermediate type/container nodes
            "generic_type"
            | "reference_type"
            | "array_type"
            | "tuple_type"
            | "pointer_type"
            | "scoped_type_identifier"
            | "type_arguments"
            | "field_declaration_list"
            | "declaration_list"
            | "enum_variant_list" => {},
            _ => {
                // Last resort: check the line before this parent for a comment
                let start_row = parent.start_position().row;
                if start_row > 0 {
                    if let Some(prev_line) = source.lines().nth(start_row - 1) {
                        if crate::line_lint_allowed(prev_line, "bare_collection") {
                            return Some(extract_explanation(prev_line, "bare_collection"));
                        }
                    }
                }
                break;
            },
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
                if crate::line_lint_allowed(comment_text, "bare_collection") {
                    found_explanation = Some(extract_explanation(comment_text, "bare_collection"));
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
            if crate::line_lint_allowed(comment_text, "bare_collection") {
                return Some(extract_explanation(comment_text, "bare_collection"));
            }
        }
    }

    None
}

/// Extract explanation text following a `lint:allow(...)` marker whose
/// parenthesised name list includes `rule_name`. Honors both the single-
/// name form `lint:allow(bare_collection)` and the comma-separated form
/// `lint:allow(bare_collection, no_box)`. Accepts separators after the
/// marker's closing paren: ` — `, ` - `, `: `, or whitespace.
fn extract_explanation(comment: &str, rule_name: &str) -> String {
    let needle = "lint:allow(";
    let mut search = comment;
    while let Some(start) = search.find(needle) {
        let after_open = &search[start + needle.len() ..];
        if let Some(close) = after_open.find(')') {
            let names = &after_open[.. close];
            if names.split(',').any(|n| n.trim() == rule_name) {
                let after = &after_open[close + 1 ..];
                let trimmed = after.trim();
                let explanation = if trimmed.starts_with("—") || trimmed.starts_with("–") {
                    trimmed[trimmed.char_indices().nth(1).map(|(i, _)| i).unwrap_or(1) ..].trim()
                } else if trimmed.starts_with('-') {
                    trimmed[1 ..].trim()
                } else if trimmed.starts_with(':') {
                    trimmed[1 ..].trim()
                } else {
                    trimmed
                };
                return explanation.to_string();
            }
            search = &after_open[close + 1 ..];
        } else {
            break;
        }
    }
    String::new()
}

/// Check if an explanation has enough words to be considered sufficient.
fn explanation_is_sufficient(explanation: &str) -> bool {
    explanation.split_whitespace().count() >= MIN_EXPLANATION_WORDS
}

/// Check if a source line has a lint:allow(bare_collection) comment (trailing or otherwise).
/// Returns Some(explanation) if found, None otherwise. Honors the comma-
/// list form `lint:allow(bare_collection, no_box)`.
fn line_allow_explanation(line: &str) -> Option<String> {
    if crate::line_lint_allowed(line, "bare_collection") {
        Some(extract_explanation(line, "bare_collection"))
    } else {
        None
    }
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

    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments but track lint:allow on comment-only lines
        if trimmed.starts_with("//") {
            if crate::line_lint_allowed(trimmed, "bare_collection") {
                prev_line_allow = true;
                prev_line_explanation = extract_explanation(trimmed, "bare_collection");
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
                let line_has_allow = crate::line_lint_allowed(line, "bare_collection");
                let has_allow = line_has_allow || prev_line_allow;
                if has_allow {
                    let explanation = if line_has_allow {
                        extract_explanation(line, "bare_collection")
                    } else {
                        prev_line_explanation.clone()
                    };
                    let (severity, message) = if explanation_is_sufficient(&explanation) {
                        (
                            crate::Severity::ADVISORY,
                            format!(
                                "suppressed by lint:allow(bare_collection) in macro: {explanation}"
                            ),
                        )
                    } else {
                        (crate::Severity::PUSH_GATE,
                         "lint:allow(bare_collection) in macro requires an explanation (8+ words)".to_string())
                    };
                    // Still emit a diagnostic so it's visible
                    for &(pattern, _) in MACRO_FORBIDDEN {
                        if trimmed.contains(pattern) {
                            errors.push(LintError {
                                path: None,
                                crate_name: ctx.crate_name.to_string(),
                                line: line_num + 1,
                                lint_name: LINT_NAME,
                                severity,
                                message: message.clone(),
                                finding_kind: None,
                            });
                            break;
                        }
                    }
                } else {
                    for &(pattern, replacement) in MACRO_FORBIDDEN {
                        if trimmed.contains(pattern) {
                            let col_name = pattern.split('<').next().unwrap_or(pattern);
                            errors.push(LintError {
                                path: None,
                                crate_name: ctx.crate_name.to_string(),
                                line: line_num + 1,
                                lint_name: LINT_NAME,
                                severity: crate::Severity::PUSH_GATE,
                                message: format!(
                                    "`{col_name}` names a container in a macro body. Name the contract instead: \
                                     declare what this position needs as a trait, take it as a \
                                     bound, and let the caller supply the container and its \
                                     allocation. {replacement}",
                                ),
                                finding_kind: None,
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// The six names an earlier version of this lint told every consumer to
    /// reach for. None of them exists in any repository this lint governs; they
    /// came from a downstream consumer's storage crate.
    const NAMES_FROM_NOWHERE: &[&str] = &[
        "Collection<T>",
        "Dictionary<K, V>",
        "DenseColumn",
        "IdMap<K, V>",
        "SparseArray",
        "SparseSet<T>",
    ];

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

    fn reported(source: &'static str) -> Vec<LintError> {
        NoBareVec.check(&ctx_for(source))
    }

    #[test]
    fn a_container_in_a_public_signature_is_reported() {
        // The control for every message test below: they would all pass
        // vacuously against a lint that reported nothing.
        assert!(!reported("pub fn f(v: Vec<u8>) {}\n").is_empty());
    }

    #[test]
    fn the_guidance_names_no_type_that_does_not_exist() {
        // The whole failure this test exists for: the lint spent its life
        // telling consumers to use six types from a repository none of them
        // depend on, so the only way to satisfy it was to ignore it.
        for message in reported("pub fn f(v: Vec<u8>) {}\n").iter().map(|e| e.to_string()) {
            for name in NAMES_FROM_NOWHERE {
                assert!(
                    !message.contains(name),
                    "the guidance names `{name}`, which exists in no governed repository: {message}",
                );
            }
            assert!(
                !message.contains("storage crate"),
                "the guidance points at a crate that does not exist here: {message}",
            );
        }
    }

    #[test]
    fn the_guidance_asks_for_a_contract_rather_than_another_container() {
        // Naming a different concrete container would repeat the mistake in a
        // new vocabulary. The instruction has to be to stop naming containers.
        let messages: Vec<String> =
            reported("pub fn f(v: Vec<u8>) {}\n").iter().map(|e| e.to_string()).collect();
        assert!(
            messages.iter().any(|m| m.contains("trait") && m.contains("bound")),
            "no message told the reader to declare a trait and take it as a bound: {messages:?}",
        );
        assert!(
            messages.iter().any(|m| m.contains("allocation")),
            "no message said who supplies the allocation: {messages:?}",
        );
    }

    #[test]
    fn no_message_carries_an_em_dash() {
        // Banned in every authored line, and a lint message is read more often
        // than most prose in the tree.
        for source in [
            "pub fn f(v: Vec<u8>) {}\n",
            "pub struct S { pub items: HashMap<u8, u8> }\n",
            "pub fn g() -> BTreeSet<u8> { todo!() }\n",
        ] {
            for message in reported(source).iter().map(|e| e.to_string()) {
                assert!(!message.contains('\u{2014}'), "em-dash in a lint message: {message}");
                assert!(!message.contains('\u{2013}'), "en-dash in a lint message: {message}");
            }
        }
    }

    #[test]
    fn a_map_and_a_set_get_different_advice_from_a_sequence() {
        // One shape per kind. Collapsing them would produce advice that is
        // correct for a Vec and wrong for the two thirds of the table that are
        // not sequences.
        let seq = reported("pub fn f(v: Vec<u8>) {}\n");
        let map = reported("pub fn f(v: HashMap<u8, u8>) {}\n");
        let set = reported("pub fn f(v: HashSet<u8>) {}\n");
        assert!(!seq.is_empty() && !map.is_empty() && !set.is_empty());

        let text = |errors: &[LintError]| -> String {
            errors.iter().map(std::string::ToString::to_string).collect()
        };
        assert!(text(&seq).contains("sequence"), "a Vec was not called a sequence");
        assert!(text(&map).contains("keyed lookup"), "a HashMap was not called a keyed lookup");
        assert!(text(&set).contains("membership"), "a HashSet was not called membership");
    }
}
