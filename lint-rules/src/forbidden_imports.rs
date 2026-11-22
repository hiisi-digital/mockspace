//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Lint: unified forbidden-imports rule.
//!
//! A single data-driven lint that replaces many individual forbidden-type lints.
//! Rules are configured via `[lints.forbidden-imports.rule.*]` in `mockspace.toml`.
//!
//! Each rule specifies:
//! - `scope`: crate glob pattern (e.g., `"*-sdk"`, `"*"`, `"{prefix}-primitives"`)
//! - `forbidden`: patterns to forbid (e.g., `"String"`, `"dyn *"`, `"std::*"`)
//! - `reason`: human-readable explanation
//! - `enabled`: optional, defaults to true
//!
//! Pattern matching:
//! - `"String"`: matches the word `String` as a type (word boundary, not in quotes)
//! - `"dyn *"`: matches `dyn ` keyword in type position
//! - `"std::*"`: matches `use std::` or `std::` in type paths. A path inside the body of a
//!   `#[macro_export]` macro is left out (also `#[cfg_attr(.., macro_export)]`): it expands in the
//!   consumer's crate, where it resolves, and not in the crate that defines the macro. `dyn` and
//!   type-name patterns are checked there as before.
//! - `"f32"`, `"f64"`: matches these as type annotations
//! - `"{prefix}_core::*"`: matches imports from sibling crates

use std::collections::HashMap;

use tree_sitter::Node;

use crate::{CrateLint, Lint, LintContext, LintError, Severity};

/// A single forbidden-imports rule.
#[derive(Clone, Debug)]
struct ForbiddenRule {
    /// Rule name (from the TOML key, e.g. "no-std-in-primitives").
    name:      String,
    /// Crate glob pattern, e.g., `"*-sdk"`, `"*"`, `"{prefix}-primitives"`.
    scope:     String,
    /// Forbidden patterns: `"String"`, `"dyn *"`, `"std::*"`, etc.
    forbidden: Vec<String>,
    /// Human-readable reason.
    reason:    String,
    /// Whether this rule is active.
    enabled:   bool,
}

pub struct ForbiddenImports {
    rules: Vec<ForbiddenRule>,
}

impl ForbiddenImports {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
        }
    }
}

impl Lint for ForbiddenImports {
    fn name(&self) -> &'static str {
        "forbidden-imports"
    }

    fn default_severity(&self) -> Severity {
        Severity::OFF
    }

    fn config_keys(&self) -> &[&str] {
        &["rules"]
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        // Rules are passed as flattened keys from config parsing:
        //   "rule.no-std-in-sdk.scope" = "{prefix}-sdk"
        //   "rule.no-std-in-sdk.forbidden" = "std::*"
        //   "rule.no-std-in-sdk.reason" = "SDK is #![no_std] + alloc"
        //   "rule.no-std-in-sdk.enabled" = "false"
        //
        // First, collect all unique rule names.
        let mut rule_names: Vec<String> = Vec::new();
        for key in params.keys() {
            if let Some(rest) = key.strip_prefix("rule.") {
                if let Some((name, _field)) = rest.rsplit_once('.') {
                    let name = name.to_string();
                    if !rule_names.contains(&name) {
                        rule_names.push(name);
                    }
                }
            }
        }
        rule_names.sort();

        for rule_name in rule_names {
            let prefix = format!("rule.{rule_name}.");
            let scope = params
                .get(&format!("{prefix}scope"))
                .cloned()
                .unwrap_or_else(|| "*".to_string());
            let forbidden_raw = params
                .get(&format!("{prefix}forbidden"))
                .cloned()
                .unwrap_or_default();
            let reason = params
                .get(&format!("{prefix}reason"))
                .cloned()
                .unwrap_or_default();
            let enabled = params
                .get(&format!("{prefix}enabled"))
                .map(|v| v != "false")
                .unwrap_or(true);

            // Forbidden can be comma-separated
            let forbidden: Vec<String> = forbidden_raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !forbidden.is_empty() {
                self.rules.push(ForbiddenRule {
                    name: rule_name,
                    scope,
                    forbidden,
                    reason,
                    enabled,
                });
            }
        }
    }
}

impl CrateLint for ForbiddenImports {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let mut errors = Vec::new();

        for rule in &self.rules {
            if !rule.enabled {
                continue;
            }

            // Check if the current crate matches the scope
            if !scope_matches(&rule.scope, ctx.crate_name, ctx.crate_prefix) {
                continue;
            }

            // Check each forbidden pattern against the source
            for pattern in &rule.forbidden {
                check_forbidden_pattern(ctx, rule, pattern, &mut errors);
            }
        }

        errors
    }
}

/// Check if a crate name matches a scope glob pattern.
///
/// Supported patterns:
/// - `"*"`: matches all crates
/// - `"*-sdk"`: crates ending in `-sdk`
/// - `"prefix-*"`: crates starting with `prefix-`
/// - `"prefix-connector-*"`: crates matching the prefix pattern
/// - Exact name: `"prefix-primitives"`
fn scope_matches(scope: &str, crate_name: &str, crate_prefix: &str) -> bool {
    // Expand {prefix} placeholder
    let scope = scope.replace("{prefix}", crate_prefix);

    if scope == "*" {
        return true;
    }

    if scope.starts_with('*') && scope.ends_with('*') {
        // *pattern*: contains
        let inner = &scope[1 .. scope.len() - 1];
        return crate_name.contains(inner);
    }

    if let Some(suffix) = scope.strip_prefix('*') {
        // *-sdk: ends with
        return crate_name.ends_with(suffix);
    }

    if let Some(prefix) = scope.strip_suffix('*') {
        // prefix-connector-*: starts with
        return crate_name.starts_with(prefix);
    }

    // Exact match
    scope == crate_name
}

/// The byte ranges of every `#[macro_export]` macro in the parse.
///
/// The tokens of an exported macro expand in the consumer's crate, so a path in its body names
/// something that resolves there and not in the crate that defines the macro. A `macro_rules!`
/// that is not exported expands in its own crate and is not listed.
fn exported_macro_ranges(root: Node, source: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    collect_exported_macros(root, source, &mut ranges);
    ranges
}

fn collect_exported_macros(node: Node, source: &str, out: &mut Vec<std::ops::Range<usize>>) {
    if node.kind() == "macro_definition" && is_exported(node, source) {
        out.push(node.byte_range());
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_exported_macros(child, source, out);
    }
}

/// Whether an attribute directly before the macro is `macro_export`, or a `cfg_attr` applying it.
///
/// Comments between the attributes are skipped and never count, and an attribute earlier than a
/// non-attribute item belongs to that item.
fn is_exported(mac: Node, source: &str) -> bool {
    let mut prev = mac.prev_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            "attribute_item" => {
                if attribute_exports(sibling, source) {
                    return true;
                }
            }
            "line_comment" | "block_comment" => {}
            _ => return false,
        }
        prev = sibling.prev_sibling();
    }
    false
}

fn attribute_exports(item: Node, source: &str) -> bool {
    let mut cursor = item.walk();
    let Some(attr) = item.children(&mut cursor).find(|c| c.kind() == "attribute") else {
        return false;
    };
    let mut cursor = attr.walk();
    let mut children = attr.children(&mut cursor);
    let Some(path) = children.next() else {
        return false;
    };
    match &source[path.byte_range()] {
        "macro_export" => true,
        "cfg_attr" => children
            .find(|c| c.kind() == "token_tree")
            .is_some_and(|args| cfg_attr_applies_export(args, source)),
        _ => false,
    }
}

/// `cfg_attr(<condition>, <attributes...>)`: whether `macro_export` is among the attributes, after the
/// first top-level comma, rather than inside the condition.
fn cfg_attr_applies_export(args: Node, source: &str) -> bool {
    let mut cursor = args.walk();
    let mut after_condition = false;
    for child in args.children(&mut cursor) {
        if child.kind() == "," {
            after_condition = true;
        } else if after_condition && names_export(child, source) {
            return true;
        }
    }
    false
}

fn names_export(node: Node, source: &str) -> bool {
    if node.kind() == "identifier" && &source[node.byte_range()] == "macro_export" {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|c| names_export(c, source))
}

/// Check a single forbidden pattern against the source text.
fn check_forbidden_pattern(
    ctx: &LintContext,
    rule: &ForbiddenRule,
    pattern: &str,
    errors: &mut Vec<LintError>,
) {
    let crate_prefix = ctx.crate_prefix;

    // Expand {prefix} in patterns (using underscore form for module paths)
    let pattern = pattern.replace("{prefix}", &crate_prefix.replace('-', "_"));

    if pattern.ends_with("::*") {
        // Module path pattern: "std::*" matches `use std::` and `std::` in paths
        let module_prefix = pattern.trim_end_matches("::*");
        check_module_path(ctx, rule, module_prefix, errors);
    } else if pattern.starts_with("dyn ") {
        // Keyword pattern: "dyn *" matches `dyn ` in type position
        check_keyword_pattern(ctx, rule, "dyn ", errors);
    } else {
        // Type name pattern: "String", "f32", "Vec", etc.
        check_type_name(ctx, rule, &pattern, errors);
    }
}

/// Check for forbidden module path imports (e.g., `std::`, `acme_core::`).
fn check_module_path(
    ctx: &LintContext,
    rule: &ForbiddenRule,
    module_prefix: &str,
    errors: &mut Vec<LintError>,
) {
    let path_pattern = format!("{module_prefix}::");
    // `use {module_prefix}::` contains the path pattern, so one search finds both.
    let source = ctx.source();
    let exported = exported_macro_ranges(ctx.tree().root_node(), source);

    let mut line_start = 0;
    for (line_num, line) in source.split_inclusive('\n').enumerate() {
        let offset = line_start;
        line_start += line.len();
        let line = line.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if crate::line_lint_allowed(line, &rule.name) {
            continue;
        }

        // Skip string literals (rough heuristic: lines starting with a quote)
        if trimmed.starts_with('"') || trimmed.starts_with("r#\"") || trimmed.starts_with("r\"") {
            continue;
        }

        // A match inside an exported macro expands in the consumer's crate, so only one outside
        // every exported macro counts.
        let hit = line
            .match_indices(&path_pattern)
            .any(|(at, _)| !exported.iter().any(|r| r.contains(&(offset + at))));
        if hit {
            errors.push(LintError::with_finding_kind(
                ctx.crate_name.to_string(),
                line_num + 1,
                "forbidden-imports",
                format!(
                    "`{module_prefix}::*` import forbidden in this crate: {}",
                    rule.reason
                ),
                Severity::HARD_ERROR,
                leak_rule_name(&rule.name),
            ));
        }
    }
}

/// Check for a forbidden keyword pattern (e.g., `dyn `).
fn check_keyword_pattern(
    ctx: &LintContext,
    rule: &ForbiddenRule,
    keyword: &str,
    errors: &mut Vec<LintError>,
) {
    for (line_num, line) in ctx.source().lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if crate::line_lint_allowed(line, &rule.name) {
            continue;
        }

        // Skip string literals
        if trimmed.starts_with('"') || trimmed.starts_with("r#\"") || trimmed.starts_with("r\"") {
            continue;
        }

        // Check for the keyword (not inside strings: rough heuristic)
        if contains_keyword(trimmed, keyword) {
            errors.push(LintError::with_finding_kind(
                ctx.crate_name.to_string(),
                line_num + 1,
                "forbidden-imports",
                format!("`{}` keyword forbidden: {}", keyword.trim(), rule.reason),
                Severity::HARD_ERROR,
                leak_rule_name(&rule.name),
            ));
        }
    }
}

/// Check for a forbidden type name (e.g., `String`, `f32`, `Vec`).
fn check_type_name(
    ctx: &LintContext,
    rule: &ForbiddenRule,
    type_name: &str,
    errors: &mut Vec<LintError>,
) {
    for (line_num, line) in ctx.source().lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if crate::line_lint_allowed(line, &rule.name) {
            continue;
        }

        // Skip string literals
        if trimmed.starts_with('"') || trimmed.starts_with("r#\"") || trimmed.starts_with("r\"") {
            continue;
        }

        // Check for the type name at word boundaries
        if contains_type_name(trimmed, type_name) {
            errors.push(LintError::with_finding_kind(
                ctx.crate_name.to_string(),
                line_num + 1,
                "forbidden-imports",
                format!(
                    "`{type_name}` type forbidden in this crate: {}",
                    rule.reason
                ),
                Severity::HARD_ERROR,
                leak_rule_name(&rule.name),
            ));
        }
    }
}

/// Check if a line contains a type name at word boundaries.
///
/// Matches `String` as a type but not inside a larger identifier like `MyString`
/// or after a dot like `.to_string()`. Also avoids matching inside string literals
/// in the middle of a line.
fn contains_type_name(line: &str, type_name: &str) -> bool {
    let bytes = line.as_bytes();
    let pat_bytes = type_name.as_bytes();
    let pat_len = pat_bytes.len();

    if bytes.len() < pat_len {
        return false;
    }

    let mut i = 0;
    while i + pat_len <= bytes.len() {
        if &bytes[i .. i + pat_len] == pat_bytes {
            // Check word boundary before
            let before_ok = if i == 0 {
                true
            } else {
                let before = bytes[i - 1];
                !is_ident_char(before) && before != b'.'
            };

            // Check word boundary after
            let after_ok = if i + pat_len >= bytes.len() {
                true
            } else {
                let after = bytes[i + pat_len];
                !is_ident_char(after)
            };

            if before_ok && after_ok {
                // Make sure we're not inside a string literal
                // (rough: count unescaped quotes before this position)
                let prefix = &line[.. i];
                let quote_count = prefix.chars().filter(|c| *c == '"').count();
                if quote_count % 2 == 0 {
                    return true;
                }
            }
        }
        i += 1;
    }

    false
}

/// Check if a line contains a keyword like `dyn ` in a code position.
fn contains_keyword(line: &str, keyword: &str) -> bool {
    let bytes = line.as_bytes();
    let pat_bytes = keyword.as_bytes();
    let pat_len = pat_bytes.len();

    if bytes.len() < pat_len {
        return false;
    }

    let mut i = 0;
    while i + pat_len <= bytes.len() {
        if &bytes[i .. i + pat_len] == pat_bytes {
            // Check word boundary before
            let before_ok = if i == 0 { true } else { !is_ident_char(bytes[i - 1]) };

            if before_ok {
                // Check we're not inside a string literal
                let prefix = &line[.. i];
                let quote_count = prefix.chars().filter(|c| *c == '"').count();
                if quote_count % 2 == 0 {
                    return true;
                }
            }
        }
        i += 1;
    }

    false
}

/// Check if a byte is a valid Rust identifier character.
fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Leak a rule name string to get a `&'static str`.
///
/// This is used for `finding_kind` which requires `&'static str`.
/// The leaked memory is negligible (rule names are small and few).
pub(crate) fn leak_rule_name(name: &str) -> &'static str {
    // Cache leaked strings to avoid leaking the same name multiple times
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);

    let mut guard = CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);

    if let Some(leaked) = cache.get(name) {
        return leaked;
    }

    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    cache.insert(name.to_string(), leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_matches() {
        assert!(scope_matches("*", "acme-sdk", "acme"));
        assert!(scope_matches("*-sdk", "acme-sdk", "acme"));
        assert!(!scope_matches("*-sdk", "acme-core", "acme"));
        assert!(scope_matches(
            "{prefix}-primitives",
            "acme-primitives",
            "acme"
        ));
        assert!(!scope_matches("{prefix}-primitives", "acme-sdk", "acme"));
        assert!(scope_matches(
            "{prefix}-connector-*",
            "acme-connector-igdb",
            "acme"
        ));
        assert!(!scope_matches("{prefix}-connector-*", "acme-sdk", "acme"));
        assert!(scope_matches("acme-sdk", "acme-sdk", "acme"));
        assert!(!scope_matches("acme-sdk", "acme-core", "acme"));
    }

    #[test]
    fn test_contains_type_name() {
        assert!(contains_type_name("pub name: String,", "String"));
        assert!(contains_type_name("fn foo() -> String {", "String"));
        assert!(contains_type_name("let x: Vec<String> = vec![];", "String"));
        assert!(!contains_type_name("let x = my_string;", "String"));
        // Comments are filtered at the caller level, not in contains_type_name
        assert!(contains_type_name("// String is good", "String"));
        assert!(contains_type_name("val: f32,", "f32"));
        assert!(!contains_type_name("val: f320,", "f32"));
        assert!(!contains_type_name("x.to_string()", "String"));
    }

    #[test]
    fn test_contains_keyword() {
        assert!(contains_keyword("fn foo(x: &dyn Bar) {", "dyn "));
        assert!(contains_keyword("Box<dyn Trait>", "dyn "));
        // Comments are filtered at the caller level, not in contains_keyword
        assert!(contains_keyword("// dyn is not allowed", "dyn "));
    }
}

#[cfg(test)]
mod exported_macro_tests {
    use super::*;
    use crate::testkit::LintFixture;

    fn lint_forbidding(forbidden: &str) -> ForbiddenImports {
        let mut lint = ForbiddenImports::new();
        let mut params = HashMap::new();
        params.insert("rule.r.scope".to_string(), "test*".to_string());
        params.insert("rule.r.forbidden".to_string(), forbidden.to_string());
        params.insert("rule.r.reason".to_string(), "no_std".to_string());
        lint.configure(&params);
        lint
    }

    fn findings_for(forbidden: &str, src: &str) -> Vec<usize> {
        let fixture = LintFixture::new(src).with_crate_name("test-crate", "test");
        let ctx = fixture.ctx();
        lint_forbidding(forbidden)
            .check(&ctx)
            .iter()
            .map(|e| e.line)
            .collect()
    }

    fn findings(src: &str) -> Vec<usize> {
        findings_for("std::*", src)
    }

    const BODY: &str = "    () => { ::std::vec::Vec::new() };\n";

    #[test]
    fn exported_macro_body_naming_std_is_not_flagged() {
        let src = "#[macro_export]\nmacro_rules! emit {\n    () => {\n        ::std::println!(\"x\")\n    };\n}\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn exported_macro_with_doc_and_other_attribute_is_not_flagged() {
        let src = format!("/// docs\n#[macro_export]\n#[doc(hidden)]\nmacro_rules! emit {{\n{BODY}}}\n");
        assert!(findings(&src).is_empty());
    }

    #[test]
    fn real_use_of_std_is_still_flagged() {
        assert_eq!(findings("use std::fmt;\n"), vec![1]);
    }

    #[test]
    fn real_use_after_an_exported_macro_is_still_flagged() {
        let src = format!("#[macro_export]\nmacro_rules! emit {{\n{BODY}}}\nuse std::fmt;\n");
        assert_eq!(findings(&src), vec![5]);
    }

    #[test]
    fn local_macro_naming_std_is_still_flagged() {
        let src = format!("macro_rules! local {{\n{BODY}}}\n");
        assert_eq!(findings(&src), vec![2]);
    }

    #[test]
    fn raw_string_with_a_quote_in_the_body_does_not_end_the_mask_early() {
        let src = "#[macro_export]\nmacro_rules! m {\n    () => { r#\"\"\"# };\n}\nuse std::fmt;\n";
        assert_eq!(findings(src), vec![5]);
    }

    #[test]
    fn raw_string_with_a_brace_in_the_body_does_not_end_the_mask_early() {
        let src = "#[macro_export]\nmacro_rules! m {\n    () => { r#\"a\"}\"# };\n}\nuse std::fmt;\n";
        assert_eq!(findings(src), vec![5]);
    }

    #[test]
    fn code_after_the_closing_brace_on_the_same_line_is_still_flagged() {
        let src = "#[macro_export]\nmacro_rules! m { () => { ::std::vec::Vec::new() }; } fn g() -> std::fmt::Result { Ok(()) }\n";
        assert_eq!(findings(src), vec![2]);
    }

    #[test]
    fn a_non_exported_macro_beside_an_exported_one_on_one_line_is_flagged() {
        let src = "#[macro_export] macro_rules! a { () => { ::std::x }; } macro_rules! b { () => { ::std::y }; }\n";
        assert_eq!(findings(src), vec![1]);
    }

    #[test]
    fn two_exported_macros_on_one_line_are_both_skipped() {
        let src = "#[macro_export] macro_rules! a { () => { ::std::x }; } #[macro_export] macro_rules! b { () => { ::std::y }; }\n";
        assert!(findings(src).is_empty());
    }

    #[test]
    fn a_commented_out_export_does_not_export_a_local_macro() {
        let src = format!("// #[macro_export]\nmacro_rules! m {{\n{BODY}}}\n");
        assert_eq!(findings(&src), vec![3]);
    }

    #[test]
    fn a_doc_comment_mentioning_the_export_does_not_export_a_local_macro() {
        let src = format!("/// Not #[macro_export]ed\nmacro_rules! m {{\n{BODY}}}\n");
        assert_eq!(findings(&src), vec![3]);
    }

    #[test]
    fn a_multi_line_cfg_attr_between_the_export_and_the_macro_keeps_it_exported() {
        let src = format!(
            "#[macro_export]\n#[cfg_attr(\n    feature = \"x\",\n    doc(hidden)\n)]\nmacro_rules! m {{\n{BODY}}}\n"
        );
        assert!(findings(&src).is_empty());
    }

    #[test]
    fn a_cfg_attr_that_applies_the_export_exports_the_macro() {
        let src = format!("#[cfg_attr(feature = \"x\", macro_export)]\nmacro_rules! m {{\n{BODY}}}\n");
        assert!(findings(&src).is_empty());
    }

    #[test]
    fn a_cfg_attr_whose_condition_names_the_export_does_not_export() {
        let src = format!("#[cfg_attr(macro_export, doc(hidden))]\nmacro_rules! m {{\n{BODY}}}\n");
        assert_eq!(findings(&src), vec![3]);
    }

    #[test]
    fn an_export_with_arguments_exports_the_macro() {
        let src = format!("#[macro_export(local_inner_macros)]\nmacro_rules! m {{\n{BODY}}}\n");
        assert!(findings(&src).is_empty());
    }

    #[test]
    fn parenthesis_and_bracket_delimited_exported_macros_are_skipped() {
        let paren = "#[macro_export]\nmacro_rules! m (\n    () => ( ::std::x )\n);\nuse std::fmt;\n";
        assert_eq!(findings(paren), vec![5]);
        let bracket = "#[macro_export]\nmacro_rules! m [\n    () => [ ::std::x ]\n];\nuse std::fmt;\n";
        assert_eq!(findings(bracket), vec![5]);
    }

    #[test]
    fn an_export_attribute_on_a_function_does_not_export_the_next_macro() {
        let src = format!("#[macro_export]\nfn f() {{}}\nmacro_rules! m {{\n{BODY}}}\n");
        assert_eq!(findings(&src), vec![4]);
    }

    #[test]
    fn dyn_in_an_exported_body_is_still_flagged() {
        let src = "#[macro_export]\nmacro_rules! m {\n    () => { Box<dyn Foo> };\n}\n";
        assert_eq!(findings_for("dyn *", src), vec![3]);
    }

    #[test]
    fn a_type_name_in_an_exported_body_is_still_flagged() {
        let src = "#[macro_export]\nmacro_rules! m {\n    () => { String };\n}\n";
        assert_eq!(findings_for("String", src), vec![3]);
    }
}
