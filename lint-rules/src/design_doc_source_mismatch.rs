//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Lint: design doc / source mismatch.
//!
//! Cross-references each crate's DESIGN.md.tmpl type/signal/macro tables with
//! the actual source exports. Flags items documented in DESIGN that do not
//! appear in source code.
//!
//! Severity: PUSH_GATE (warn on commit/build, block push). This is why
//! SHAME.md.tmpl exists: document known gaps that cannot be fixed yet.
//!
//! Crates with no DESIGN.md.tmpl are skipped. Nuked crates (containing the
//! xtask nuke marker) are skipped: there is no source to compare against.

use tree_sitter::Node;

use crate::{CrateLint, Lint, LintContext, LintError};

const LINT_NAME: &str = "design-doc-source-mismatch";

/// Node kinds that define a named item in source.
const NAMED_ITEM_KINDS: &[&str] = &[
    "struct_item",
    "enum_item",
    "trait_item",
    "function_item",
    "function_signature_item",
    "const_item",
    "static_item",
    "type_item",
    "macro_definition",
];

pub struct DesignDocSourceMismatch;

impl Lint for DesignDocSourceMismatch {
    /// Crate-scoped. It compares the design document against the whole crate
    /// surface. Per file, every name declared elsewhere would read as missing.
    fn per_file(&self) -> bool {
        false
    }

    fn default_severity(&self) -> crate::Severity {
        crate::Severity::PUSH_GATE
    }

    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl CrateLint for DesignDocSourceMismatch {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let design_doc = match ctx.design_doc {
            Some(d) => d,
            None => return Vec::new(),
        };

        // Skip nuked crates: no source to compare against
        if ctx.source.contains("Nuked by") {
            return Vec::new();
        }

        // Collect all named items from every file in the crate. The lint is
        // crate-scoped, so `ctx.source` is only `src/lib.rs`; a type defined in
        // a module file and not named in the root would otherwise read as
        // missing from source.
        let mut source_names: Vec<String> = Vec::new();
        collect_names(ctx.tree.root_node(), ctx.source, &mut source_names);
        collect_macro_generated_names(ctx.source, &mut source_names);
        let mut parser = crate::make_parser();
        for file in ctx.all_sources {
            if let Some(tree) = parser.parse(&file.text, None) {
                collect_names(tree.root_node(), &file.text, &mut source_names);
            }
            collect_macro_generated_names(&file.text, &mut source_names);
        }
        let in_any_file = |name: &str| {
            source_contains_name(ctx.source, name)
                || ctx.all_sources.iter().any(|f| source_contains_name(&f.text, name))
        };

        // Extract type names from DESIGN.md.tmpl tables
        let design_names = extract_design_type_names(design_doc);

        let mut errors = Vec::new();

        for (name, line) in &design_names {
            // Skip common non-type table entries
            if is_table_noise(name) {
                continue;
            }

            // Check if the name appears in source (as a defined item or
            // inside a macro invocation that would generate it)
            let found =
                source_names.iter().any(|s| s == name) || in_any_file(name);

            if found {
                continue;
            }

            // Escape hatch: SHAME.md.tmpl entry with a 50+ word explanation.
            // Mirrors the `undocumented-type` lint's SHAME handling so
            // consumers have one consistent way to document known gaps.
            if let Some(shame) = ctx.shame_doc {
                if let Some(explanation) = find_shame_entry(shame, name) {
                    let word_count = explanation.split_whitespace().count();
                    if word_count >= 50 {
                        continue; // sufficient SHAME entry: silenced
                    }
                    errors.push(LintError::push_error(
                        ctx.crate_name.to_string(),
                        *line,
                        LINT_NAME,
                        format!(
                            "`{name}` documented in DESIGN.md.tmpl but not found in source; \
                             SHAME.md.tmpl entry only {word_count}/50 words: expand it or \
                             remove `{name}` from DESIGN.md.tmpl."
                        ),
                    ));
                    continue;
                }
            }

            errors.push(LintError::push_error(
                ctx.crate_name.to_string(),
                *line,
                LINT_NAME,
                format!(
                    "`{name}` documented in DESIGN.md.tmpl but not found in source: \
                     add it to source, remove from DESIGN, or add a SHAME.md.tmpl entry \
                     (`## {name}` header + 50+ word explanation).",
                ),
            ));
        }

        errors
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
///
/// Matches the layout documented in `undocumented_type`.
fn find_shame_entry<'a>(shame_content: &'a str, type_name: &str) -> Option<&'a str> {
    let header = format!("## {type_name}");
    let start = shame_content.find(&header)?;
    let after_header = &shame_content[start + header.len() ..];
    let end = after_header.find("\n## ").unwrap_or(after_header.len());
    let entry = after_header[.. end].trim();
    if entry.is_empty() { None } else { Some(entry) }
}

/// Collect all named items from the AST.
fn collect_names(node: Node, source: &str, out: &mut Vec<String>) {
    for &kind in NAMED_ITEM_KINDS {
        if node.kind() == kind {
            if let Some(name) = find_item_name(node, source) {
                out.push(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_names(child, source, out);
    }
}

fn find_item_name(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type_identifier" || child.kind() == "identifier" {
            return Some(source[child.byte_range()].to_string());
        }
    }
    None
}

/// Extract names generated by `define_*!` macro invocations.
/// These macros typically take a name as their first argument.
fn collect_macro_generated_names(source: &str, out: &mut Vec<String>) {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("define_") && trimmed.contains('!') {
            // Extract the name after the macro invocation
            // Pattern: define_foo!(Name ... or define_foo! { Name ...
            if let Some(after_bang) = trimmed.split('!').nth(1) {
                let inner = after_bang
                    .trim_start_matches('(')
                    .trim_start_matches('{')
                    .trim();
                // First token is typically the type name
                let name = inner
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    out.push(name.to_string());
                }
            }
        }
    }
}

/// Extract type names from DESIGN.md.tmpl markdown tables.
///
/// Looks for table rows with backtick-wrapped names in the first column:
/// `| \`TypeName\` | description |`
///
/// Also catches trait names like `\`Signal\` trait`.
fn extract_design_type_names(doc: &str) -> Vec<(String, usize)> {
    let mut names = Vec::new();

    for (line_num, line) in doc.lines().enumerate() {
        let trimmed = line.trim();

        // Must be a table row (starts with |)
        if !trimmed.starts_with('|') {
            continue;
        }

        // Skip header separator rows
        if trimmed.contains("---") {
            continue;
        }

        // Skip header rows (heuristic: contains "Type" or "Purpose" as header)
        if trimmed.contains("| Type |") || trimmed.contains("| Purpose |") {
            continue;
        }

        // Extract backtick-wrapped names from the first cell
        let cells: Vec<&str> = trimmed.split('|').collect();
        if cells.len() < 2 {
            continue;
        }

        // First non-empty cell (cells[0] is empty because line starts with |)
        let first_cell = cells.get(1).unwrap_or(&"").trim();

        // Extract name from backticks
        if let Some(name) = extract_backtick_name(first_cell) {
            // Filter: must start with uppercase (type/trait names)
            if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                names.push((name, line_num + 1));
            }
        }
    }

    names
}

/// Extract a name from backtick-wrapped text like `\`TypeName\`` or `\`Signal\` trait`.
fn extract_backtick_name(cell: &str) -> Option<String> {
    let start = cell.find('`')?;
    let rest = &cell[start + 1 ..];
    let end = rest.find('`')?;
    let inside = &rest[.. end];

    // Handle macro syntax like `define_id!(Name)`: extract the macro name
    if inside.starts_with("define_") {
        // This is a macro usage example, not a type name
        return None;
    }

    // Strip trait/struct/enum suffix: `Signal trait` → `Signal`
    let name = inside.split_whitespace().next().unwrap_or(inside);

    // Strip generic params: `Collection<T>` → `Collection`
    let name = name.split('<').next().unwrap_or(name);

    // Strip parentheses: `BufferMode(...)` → `BufferMode`
    let name = name.split('(').next().unwrap_or(name);

    if name.is_empty() {
        return None;
    }

    Some(name.to_string())
}

/// Check if a name is just table noise (common words that appear in table cells
/// but are not type names).
fn is_table_noise(name: &str) -> bool {
    const NOISE: &[&str] = &[
        "Type", "Purpose", "Mode", "Field", "Backend", "None", "Some", "Self", "Ok", "Err", "Read",
        "Write", "Note", "Example", "CPU", "GPU", "API", "FFI", "DSL",
    ];
    NOISE.contains(&name)
}

/// Fallback: check if the source text contains the name anywhere (covers
/// re-exports, type aliases, and other patterns the AST walk might miss).
fn source_contains_name(source: &str, name: &str) -> bool {
    source.contains(name)
}

#[cfg(test)]
mod whole_crate_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::CrateSourceFile;

    const DESIGN: &str = "| Type | Purpose |\n|---|---|\n| `OpenChest` | the chest held open |\n";

    /// A crate whose root declares a module and whose module defines the type the design names.
    fn ctx(root: &'static str, module: &'static str) -> LintContext<'static> {
        let mut parser = crate::make_parser();
        let tree = parser.parse(root, None).unwrap();
        let files: &'static [CrateSourceFile] = Box::leak(Box::new([
            CrateSourceFile { rel_path: "src/lib.rs".into(), text: root.to_string() },
            CrateSourceFile { rel_path: "src/chest.rs".into(), text: module.to_string() },
        ]));
        LintContext {
            crate_name: "test-crate",
            short_name: "test-crate",
            source: root,
            tree: Box::leak(Box::new(tree)),
            all_sources: files,
            deps: &[],
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: Some(DESIGN),
            all_doc_content: DESIGN,
            shame_doc: None,
            workspace_root: std::path::Path::new("/tmp"),
            proc_macro_crates: &[],
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(BTreeMap::new())),
        }
    }

    #[test]
    fn a_type_defined_in_a_module_file_is_found() {
        let c = ctx("pub mod chest;\n", "pub struct OpenChest(pub Option<u8>);\n");
        assert!(DesignDocSourceMismatch.check(&c).is_empty());
    }

    #[test]
    fn a_type_defined_nowhere_in_the_crate_is_still_reported() {
        let c = ctx("pub mod chest;\n", "pub struct SomethingElse;\n");
        assert_eq!(DesignDocSourceMismatch.check(&c).len(), 1);
    }
}
