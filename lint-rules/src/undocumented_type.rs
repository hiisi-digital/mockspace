//! Cross-crate lint: every source type must appear in at least one doc file.
//!
//! Two-pass lint:
//! 1. Collect all type names (struct, enum, trait) from ALL doc files across
//!    ALL crates (README.md.tmpl, DESIGN.md.tmpl, DEEPDIVE_*.md.tmpl).
//! 2. For each crate's source, check that every defined type appears in
//!    that collected set.
//!
//! Types not found in any doc block build and push. On commit, they warn
//! prominently but do not block (to allow local iteration).
//!
//! If the crate has a SHAME.md.tmpl entry for the type (50+ word explanation),
//! the severity is downgraded to advisory (warn everywhere, never blocks).
//!
//! Severity: BUILD_GATE (commit=warn, build=error, push=error) by default.
//! ADVISORY (commit=warn, build=warn, push=warn) with sufficient SHAME entry.
//!
//! Exempt: proc-macro crates (their types are parse-time internals),
//! types inside `macro_rules!` bodies.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::{Lint, WorkspaceLint, LintContext, LintError};

const LINT_NAME: &str = "undocumented-type";

pub struct UndocumentedType;

impl Lint for UndocumentedType {
    fn default_severity(&self) -> crate::Severity {
        crate::Severity::BUILD_GATE
    }
    fn name(&self) -> &'static str {
        LINT_NAME
    }
    fn source_only(&self) -> bool {
        false
    }
}

impl WorkspaceLint for UndocumentedType {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        // Pass 1: collect all type names mentioned in ALL doc files.
        let mut documented_types: HashSet<String> = HashSet::new();
        for &(_, ctx) in crates {
            collect_type_names_from_docs(ctx.all_doc_content, &mut documented_types);
            if let Some(design) = ctx.design_doc {
                collect_type_names_from_docs(design, &mut documented_types);
            }
            if let Some(shame) = ctx.shame_doc {
                collect_type_names_from_docs(shame, &mut documented_types);
            }
        }

        // Pass 2: check each crate's source types against the documented set.
        let mut errors = Vec::new();
        for &(crate_name, ctx) in crates {
            // Skip proc-macro crate source unless the project opted into
            // linting proc-macro source (mockspace.toml
            // `lint_proc_macro_source = true`).
            if ctx.should_skip_proc_macro_source_lint() {
                continue;
            }

            let source_types = collect_source_type_defs(ctx);
            for (type_name, line) in source_types {
                if documented_types.contains(&type_name) {
                    continue;
                }

                // Check SHAME.md.tmpl for an entry.
                if let Some(shame) = ctx.shame_doc {
                    if let Some(explanation) = find_shame_entry(shame, &type_name) {
                        let word_count = explanation.split_whitespace().count();
                        if word_count >= 50 {
                            errors.push(LintError::warning(
                                crate_name.to_string(),
                                line,
                                LINT_NAME,
                                format!(
                                    "`{type_name}` is in SHAME.md.tmpl ({word_count} words) — \
                                     add it to a design doc via a design round to resolve",
                                ),
                            ));
                            continue;
                        } else {
                            errors.push(LintError::build_error(
                                crate_name.to_string(),
                                line,
                                LINT_NAME,
                                format!(
                                    "`{type_name}` SHAME.md.tmpl entry too short ({word_count}/50 words) — \
                                     explain why it exists and how it will be documented. \
                                     BLOCKS BUILD AND PUSH until fixed.",
                                ),
                            ));
                            continue;
                        }
                    }
                }

                errors.push(LintError::build_error(
                    crate_name.to_string(),
                    line,
                    LINT_NAME,
                    format!(
                        "`{type_name}` not mentioned in any design doc. \
                         BLOCKS BUILD AND PUSH. Fix: add to DESIGN.md.tmpl via a design round, \
                         or create a SHAME.md.tmpl entry (50+ words) to unblock temporarily.",
                    ),
                ));
            }
        }

        errors
    }
}

/// Collect CamelCase identifiers from doc content that look like type names.
///
/// We match any word that starts with an uppercase letter and contains at
/// least one lowercase letter (to distinguish from ALL_CAPS constants).
/// This is intentionally broad: false positives in docs are fine because
/// the set is used for inclusion checks (type must appear in SOME doc).
fn collect_type_names_from_docs(content: &str, out: &mut HashSet<String>) {
    for word in content.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if is_type_name(word) {
            out.insert(word.to_string());
        }
    }
}

/// A type name: starts with uppercase, contains at least one lowercase letter,
/// at least 2 chars, not ALL_CAPS.
fn is_type_name(word: &str) -> bool {
    if word.len() < 2 {
        return false;
    }
    let first = word.chars().next().unwrap_or('a');
    if !first.is_uppercase() {
        return false;
    }
    // Must contain at least one lowercase letter (not ALL_CAPS).
    word.chars().any(|c| c.is_lowercase())
}

/// Collect all struct/enum/trait definitions from a crate's source.
// FIXME: walks the root tree only, so an undocumented pub item in a module
// file is never seen; the lint whose whole subject is public items still
// misses most of them in a multi-file crate. tracked: #33
fn collect_source_type_defs(ctx: &LintContext) -> Vec<(String, usize)> {
    let mut defs = Vec::new();
    let root = ctx.tree.root_node();
    collect_defs(root, ctx.source, &mut defs);
    defs
}

fn collect_defs(node: Node, source: &str, out: &mut Vec<(String, usize)>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Skip macro_definition bodies.
        if child.kind() == "macro_definition" {
            continue;
        }

        // Skip test modules. A fixture declared to exercise a trait is not
        // public surface: it has no consumer, and demanding it appear in a
        // design document would put test scaffolding in the shipping
        // contract.
        if crate::is_cfg_test_mod(child, source) {
            continue;
        }

        match child.kind() {
            "struct_item" | "enum_item" | "trait_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = source[name_node.byte_range()].to_string();
                    let line = child.start_position().row + 1;

                    // Check for lint:allow suppression.
                    let line_text = source.lines().nth(child.start_position().row).unwrap_or("");
                    if !crate::line_lint_allowed(line_text, "undocumented_type") {
                        out.push((name, line));
                    }
                }
            },
            _ => {},
        }

        if child.named_child_count() > 0 {
            collect_defs(child, source, out);
        }
    }
}

/// Find a SHAME.md.tmpl entry for a type and return its explanation text.
///
/// Expected format:
/// ```markdown
/// ## TypeName
///
/// Explanation paragraph (50+ words)...
/// ```
fn find_shame_entry<'a>(shame_content: &'a str, type_name: &str) -> Option<&'a str> {
    let header = format!("## {type_name}");
    let start = shame_content.find(&header)?;
    let after_header = &shame_content[start + header.len() ..];

    // Find the next `## ` header or end of file.
    let end = after_header.find("\n## ").unwrap_or(after_header.len());

    let entry = after_header[.. end].trim();
    if entry.is_empty() { None } else { Some(entry) }
}

#[cfg(test)]
mod cfg_test_module_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// Build a context whose tree is parsed from the source under test.
    /// `file_size`'s helper parses an empty string, which is fine for a lint
    /// that only counts lines and useless for one that walks the AST.
    fn ctx_for(source: &'static str, design: &'static str) -> LintContext<'static> {
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
            design_doc:              Some(design),
            all_doc_content:         design,
            shame_doc:               None,
            workspace_root:          std::path::Path::new("/tmp"),
            proc_macro_crates:       &[],
            crate_prefix:            "test",
            lint_proc_macro_source:  false,
            primitive_introductions: Box::leak(Box::new(BTreeMap::new())),
        }
    }

    fn reported(source: &'static str, design: &'static str) -> Vec<String> {
        let ctx = ctx_for(source, design);
        UndocumentedType
            .check_all(&[("test-crate", &ctx)])
            .into_iter()
            .map(|e| e.message)
            .collect()
    }

    #[test]
    fn a_fixture_inside_a_test_module_is_not_public_surface() {
        // The case kolli hit: two structs implementing a trait so the type-level
        // list could be exercised. Neither has a consumer, and the only ways to
        // satisfy the lint were to put test scaffolding in a design document or
        // to write a SHAME entry for it.
        let reports = reported(
            "#[cfg(test)]\nmod tests {\n    struct Fixture;\n    enum Shape { A }\n}\n",
            "# test-crate\n\nNothing documented here.\n",
        );
        assert!(
            reports.is_empty(),
            "test fixtures were reported as undocumented public types: {reports:?}"
        );
    }

    #[test]
    fn an_undocumented_type_outside_a_test_module_is_still_reported() {
        // The control. Without it, a lint that reported nothing at all would
        // satisfy the test above.
        let reports = reported("pub struct Real;\n", "# test-crate\n\nNothing documented here.\n");
        assert!(
            reports.iter().any(|m| m.contains("Real")),
            "the lint stopped reporting real undocumented types: {reports:?}"
        );
    }

    #[test]
    fn a_module_that_is_not_test_only_is_still_walked() {
        // `not(test)` reads as a test module to a substring search, which would
        // hide every type in an ordinary module behind it.
        let reports = reported(
            "#[cfg(not(test))]\nmod real {\n    pub struct Hidden;\n}\n",
            "# test-crate\n\nNothing documented here.\n",
        );
        assert!(
            reports.iter().any(|m| m.contains("Hidden")),
            "a `cfg(not(test))` module was skipped as if it were a test module: {reports:?}"
        );
    }

    #[test]
    fn a_documented_type_is_not_reported() {
        let reports = reported("pub struct Real;\n", "# test-crate\n\nThe `Real` type.\n");
        assert!(reports.is_empty(), "a documented type was reported: {reports:?}");
    }
}
