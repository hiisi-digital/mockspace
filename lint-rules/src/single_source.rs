//! Cross-crate lint: one definition per concept.
//!
//! Detects the same type name (struct, enum, trait) defined in more than one
//! crate. Re-exports (`pub use`) are excluded — only actual definitions count.
//!
//! Exempt: test modules, macro-generated types (inside `macro_rules!`), and
//! type aliases (`type Foo = ...`) which are intentional re-definitions.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::{CrossCrateLint, LintContext, LintError};

/// Pairs of crate suffixes where duplicate type names are expected.
/// Macro-util crates and their parent crates often define matching types
/// at different abstraction levels (parsing vs. runtime).
/// Full crate names are built as `<prefix>-<suffix>` at check time.
const EXEMPT_PAIRS: &[(&str, &str)] = &[
    ("macro-util", "behavior"),
    ("macro-util", "resource"),
    ("macro-util", "signal"),
];

pub struct SingleSource;

impl CrossCrateLint for SingleSource {
    fn name(&self) -> &'static str {
        "single-source"
    }

    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        // Get crate prefix from the first context (all share the same prefix).
        let prefix = crates.first().map(|(_, ctx)| ctx.crate_prefix).unwrap_or("");

        // Collect all type definitions across all crates
        let mut all_defs: Vec<TypeDef> = Vec::new();

        for &(crate_name, ctx) in crates {
            let root = ctx.tree.root_node();
            collect_type_defs(root, ctx.source, crate_name, &mut all_defs);
        }

        let mut errors = Vec::new();

        // Group by name
        let mut by_name: HashMap<&str, Vec<&TypeDef>> = HashMap::new();
        for def in &all_defs {
            by_name.entry(def.name.as_str()).or_default().push(def);
        }

        // Build full exempt pairs from prefix + suffixes
        let exempt_full: Vec<(String, String)> = EXEMPT_PAIRS
            .iter()
            .map(|(a, b)| (format!("{prefix}-{a}"), format!("{prefix}-{b}")))
            .collect();

        for (name, defs) in &by_name {
            if defs.len() < 2 {
                continue;
            }

            // Must come from different crates
            let mut seen_crates: Vec<&str> = defs.iter().map(|d| d.crate_name.as_str()).collect();
            seen_crates.sort();
            seen_crates.dedup();
            if seen_crates.len() < 2 {
                continue;
            }

            let first = defs[0];
            for dup in &defs[1..] {
                if dup.crate_name != first.crate_name {
                    // Skip exempt pairs (e.g., macro-util mirrors parent crate types)
                    let is_exempt = exempt_full.iter().any(|(a, b)| {
                        (dup.crate_name == *a && first.crate_name == *b)
                            || (dup.crate_name == *b && first.crate_name == *a)
                    });
                    if is_exempt {
                        continue;
                    }
                    errors.push(LintError {
                        crate_name: dup.crate_name.clone(),
                        line: dup.line,
                        lint_name: "single-source",
                        severity: crate::Severity::HARD_ERROR,
                        message: format!(
                            "{} `{name}` also defined in {} — one definition per concept",
                            dup.kind, first.crate_name,
                        ),
                        finding_kind: None,
                    });
                }
            }
        }

        errors
    }
}

struct TypeDef {
    crate_name: String,
    name: String,
    kind: &'static str, // "struct", "enum", "trait"
    line: usize,
}

const TYPE_DEF_KINDS: &[(&str, &str)] = &[
    ("struct_item", "struct"),
    ("enum_item", "enum"),
    ("trait_item", "trait"),
];

fn collect_type_defs(root: Node, source: &str, crate_name: &str, out: &mut Vec<TypeDef>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        // Skip macro_definition (macro_rules! bodies)
        if child.kind() == "macro_definition" {
            continue;
        }

        // Skip #[cfg(test)] modules
        if child.kind() == "mod_item" && has_cfg_test(child, source) {
            continue;
        }

        for &(ast_kind, label) in TYPE_DEF_KINDS {
            if child.kind() == ast_kind {
                if let Some(name) = find_name(child, source) {
                    out.push(TypeDef {
                        crate_name: crate_name.to_string(),
                        name,
                        kind: label,
                        line: child.start_position().row + 1,
                    });
                }
            }
        }
    }
}

fn find_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier" || child.kind() == "identifier" {
            return Some(source[child.byte_range()].to_string());
        }
    }
    None
}

fn has_cfg_test(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "attribute" {
            let text = &source[child.byte_range()];
            if text.contains("cfg") && text.contains("test") {
                return true;
            }
        }
    }
    false
}
