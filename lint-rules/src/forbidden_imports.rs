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
//! - `"String"` — matches the word `String` as a type (word boundary, not in quotes)
//! - `"dyn *"` — matches `dyn ` keyword in type position
//! - `"std::*"` — matches `use std::` or `std::` in type paths
//! - `"f32"`, `"f64"` — matches these as type annotations
//! - `"{prefix}_core::*"` — matches imports from sibling crates

use std::collections::HashMap;

use crate::{Lint, LintContext, LintError, Severity};

/// A single forbidden-imports rule.
#[derive(Clone, Debug)]
struct ForbiddenRule {
    /// Rule name (from the TOML key, e.g. "no-std-in-primitives").
    name: String,
    /// Crate glob pattern, e.g., `"*-sdk"`, `"*"`, `"{prefix}-primitives"`.
    scope: String,
    /// Forbidden patterns: `"String"`, `"dyn *"`, `"std::*"`, etc.
    forbidden: Vec<String>,
    /// Human-readable reason.
    reason: String,
    /// Whether this rule is active.
    enabled: bool,
}

pub struct ForbiddenImports {
    rules: Vec<ForbiddenRule>,
}

impl ForbiddenImports {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
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
/// - `"*"` — matches all crates
/// - `"*-sdk"` — crates ending in `-sdk`
/// - `"prefix-*"` — crates starting with `prefix-`
/// - `"prefix-connector-*"` — crates matching the prefix pattern
/// - Exact name: `"prefix-primitives"`
fn scope_matches(scope: &str, crate_name: &str, crate_prefix: &str) -> bool {
    // Expand {prefix} placeholder
    let scope = scope.replace("{prefix}", crate_prefix);

    if scope == "*" {
        return true;
    }

    if scope.starts_with('*') && scope.ends_with('*') {
        // *pattern* — contains
        let inner = &scope[1..scope.len() - 1];
        return crate_name.contains(inner);
    }

    if let Some(suffix) = scope.strip_prefix('*') {
        // *-sdk — ends with
        return crate_name.ends_with(suffix);
    }

    if let Some(prefix) = scope.strip_suffix('*') {
        // prefix-connector-* — starts with
        return crate_name.starts_with(prefix);
    }

    // Exact match
    scope == crate_name
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

/// Check for forbidden module path imports (e.g., `std::`, `loimu_core::`).
fn check_module_path(
    ctx: &LintContext,
    rule: &ForbiddenRule,
    module_prefix: &str,
    errors: &mut Vec<LintError>,
) {
    let use_pattern = format!("use {module_prefix}::");
    let path_pattern = format!("{module_prefix}::");

    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if line.contains(&format!("lint:allow({})", rule.name)) {
            continue;
        }

        // Skip string literals (rough heuristic: lines starting with a quote)
        if trimmed.starts_with('"') || trimmed.starts_with("r#\"") || trimmed.starts_with("r\"") {
            continue;
        }

        // Check for use statements
        if trimmed.contains(&use_pattern) || trimmed.contains(&path_pattern) {
            errors.push(LintError::with_finding_kind(
                ctx.crate_name.to_string(),
                line_num + 1,
                "forbidden-imports",
                format!(
                    "`{module_prefix}::*` import forbidden in this crate — {}",
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
    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if line.contains(&format!("lint:allow({})", rule.name)) {
            continue;
        }

        // Skip string literals
        if trimmed.starts_with('"') || trimmed.starts_with("r#\"") || trimmed.starts_with("r\"") {
            continue;
        }

        // Check for the keyword (not inside strings — rough heuristic)
        if contains_keyword(trimmed, keyword) {
            errors.push(LintError::with_finding_kind(
                ctx.crate_name.to_string(),
                line_num + 1,
                "forbidden-imports",
                format!(
                    "`{}` keyword forbidden — {}",
                    keyword.trim(),
                    rule.reason
                ),
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
    for (line_num, line) in ctx.source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }

        // Skip lint:allow
        if line.contains(&format!("lint:allow({})", rule.name)) {
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
                    "`{type_name}` type forbidden in this crate — {}",
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
        if &bytes[i..i + pat_len] == pat_bytes {
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
                let prefix = &line[..i];
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
        if &bytes[i..i + pat_len] == pat_bytes {
            // Check word boundary before
            let before_ok = if i == 0 {
                true
            } else {
                !is_ident_char(bytes[i - 1])
            };

            if before_ok {
                // Check we're not inside a string literal
                let prefix = &line[..i];
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
fn leak_rule_name(name: &str) -> &'static str {
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
        assert!(scope_matches("*", "loimu-sdk", "loimu"));
        assert!(scope_matches("*-sdk", "loimu-sdk", "loimu"));
        assert!(!scope_matches("*-sdk", "loimu-core", "loimu"));
        assert!(scope_matches("{prefix}-primitives", "loimu-primitives", "loimu"));
        assert!(!scope_matches("{prefix}-primitives", "loimu-sdk", "loimu"));
        assert!(scope_matches("{prefix}-connector-*", "loimu-connector-igdb", "loimu"));
        assert!(!scope_matches("{prefix}-connector-*", "loimu-sdk", "loimu"));
        assert!(scope_matches("loimu-sdk", "loimu-sdk", "loimu"));
        assert!(!scope_matches("loimu-sdk", "loimu-core", "loimu"));
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
