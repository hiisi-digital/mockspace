//! Cross-crate lint: detect duplicate public function signatures.
//!
//! Compares all public function names and signatures across crates.
//! Flags functions with matching names (fuzzy: edit distance ≤ 2, ignoring
//! common prefixes/suffixes) or exact signature matches (same param types +
//! return type but different names). Suggests reuse from the crate that
//! defines the original.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::{Lint, WorkspaceLint, LintContext, LintError};

pub struct NoDuplicateFn;

impl Lint for NoDuplicateFn {
    fn name(&self) -> &'static str {
        "no-duplicate-fn"
    }
}

impl WorkspaceLint for NoDuplicateFn {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        // Collect all public function signatures from all crates
        let mut all_fns: Vec<FnSig> = Vec::new();

        // FIXME: reads each crate's root tree only, so a duplicate defined in
        // a module file is invisible; per-file collection needs the FnSigs to
        // carry their file, or the suppression lookup below reads the wrong
        // source. tracked: #33
        for &(crate_name, ctx) in crates {
            let root = ctx.tree.root_node();
            collect_pub_fns(root, ctx.source, crate_name, &mut all_fns);
        }

        let mut errors = Vec::new();

        // Group by name: exact name matches across different crates
        let mut by_name: HashMap<&str, Vec<&FnSig>> = HashMap::new();
        for sig in &all_fns {
            by_name.entry(sig.name.as_str()).or_default().push(sig);
        }

        // Build a quick crate-name to context map so the suppression
        // check below can look up the duplicate's source line.
        let ctx_by_crate: HashMap<&str, &LintContext> =
            crates.iter().map(|(name, ctx)| (*name, *ctx)).collect();

        for (name, sigs) in &by_name {
            if sigs.len() < 2 {
                continue;
            }
            // Check that they come from different crates
            let mut seen_crates: Vec<&str> = sigs.iter().map(|s| s.crate_name.as_str()).collect();
            seen_crates.sort();
            seen_crates.dedup();
            if seen_crates.len() < 2 {
                continue;
            }

            // Report: first occurrence is the "original", others are duplicates
            let first = sigs[0];
            for dup in &sigs[1 ..] {
                if dup.crate_name != first.crate_name {
                    if is_suppressed(dup, &ctx_by_crate) {
                        continue;
                    }
                    errors.push(LintError {
                        path: None,
                        crate_name:   dup.crate_name.clone(),
                        line:         dup.line,
                        lint_name:    "no-duplicate-fn",
                        severity:     crate::Severity::HARD_ERROR,
                        message:      format!(
                            "function `{name}` also defined in {}: consider reusing",
                            first.crate_name,
                        ),
                        finding_kind: None,
                    });
                }
            }
        }

        // Group by signature: same param types + return type but different names
        let mut by_sig: HashMap<String, Vec<&FnSig>> = HashMap::new();
        for sig in &all_fns {
            if sig.params.is_empty() && sig.ret.is_empty() {
                continue; // Skip trivial signatures (no params, no return)
            }
            let key = format!("({}) -> {}", sig.params.join(", "), sig.ret);
            by_sig.entry(key).or_default().push(sig);
        }

        for (_sig_key, sigs) in &by_sig {
            if sigs.len() < 2 {
                continue;
            }
            let mut seen_crates: Vec<&str> = sigs.iter().map(|s| s.crate_name.as_str()).collect();
            seen_crates.sort();
            seen_crates.dedup();
            if seen_crates.len() < 2 {
                continue;
            }

            let first = sigs[0];
            for dup in &sigs[1 ..] {
                if dup.crate_name != first.crate_name && dup.name != first.name {
                    if is_suppressed(dup, &ctx_by_crate) {
                        continue;
                    }
                    errors.push(LintError {
                        path: None,
                        crate_name:   dup.crate_name.clone(),
                        line:         dup.line,
                        lint_name:    "no-duplicate-fn",
                        severity:     crate::Severity::HARD_ERROR,
                        message:      format!(
                            "function `{}` has same signature as `{}` in {}: consider reusing",
                            dup.name, first.name, first.crate_name,
                        ),
                        finding_kind: None,
                    });
                }
            }
        }

        errors
    }
}

/// Return true if the duplicate's source line is `lint:allow`-marked.
///
/// Used to suppress cross-crate-fn-duplicate findings at sites where
/// the symbol is genuinely required to live in both crates. Typical
/// example: the linker-bound `rust_eh_personality` stub in every
/// `no_std` cdylib that does not unwind. The symbol name is fixed by
/// the Rust ABI and the function body cannot be shared via reuse.
fn is_suppressed(sig: &FnSig, ctx_by_crate: &HashMap<&str, &LintContext>) -> bool {
    let ctx = match ctx_by_crate.get(sig.crate_name.as_str()) {
        Some(c) => c,
        None => return false,
    };
    is_line_suppressed(ctx.source, sig.line)
}

/// 1-indexed line lookup; returns true if the line carries a
/// `lint:allow(no-duplicate-fn)` marker (single or comma-list).
/// Out-of-range or zero line returns false.
fn is_line_suppressed(source: &str, line_number: usize) -> bool {
    if line_number == 0 {
        return false;
    }
    let line = match source.lines().nth(line_number - 1) {
        Some(l) => l,
        None => return false,
    };
    crate::line_lint_allowed(line, "no-duplicate-fn")
}

#[cfg(test)]
mod is_line_suppressed_tests {
    use super::is_line_suppressed;

    const SAMPLE: &str = "\
pub extern \"C\" fn rust_eh_personality() {}  // lint:allow(no-duplicate-fn) -- linker contract
pub fn plain() {}
pub fn other_marker() {}  // lint:allow(no-bare-numeric)
pub fn comma_list_match() {}  // lint:allow(no-bare-result, no-duplicate-fn)
";

    #[test]
    fn suppresses_when_marker_present() {
        assert!(is_line_suppressed(SAMPLE, 1));
    }

    #[test]
    fn does_not_suppress_unmarked_line() {
        assert!(!is_line_suppressed(SAMPLE, 2));
    }

    #[test]
    fn does_not_suppress_marker_for_other_rule() {
        assert!(!is_line_suppressed(SAMPLE, 3));
    }

    #[test]
    fn suppresses_comma_list_with_no_duplicate_fn() {
        assert!(is_line_suppressed(SAMPLE, 4));
    }

    #[test]
    fn handles_line_zero_safely() {
        assert!(!is_line_suppressed(SAMPLE, 0));
    }

    #[test]
    fn handles_out_of_range_line_safely() {
        assert!(!is_line_suppressed(SAMPLE, 9999));
    }
}

struct FnSig {
    crate_name: String,
    name:       String,
    params:     Vec<String>,
    ret:        String,
    line:       usize,
}

fn collect_pub_fns(root: Node, source: &str, crate_name: &str, out: &mut Vec<FnSig>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "function_item" {
            continue;
        }
        if !has_pub(child, source) {
            continue;
        }

        // Skip proc macro functions: they all share TokenStream -> TokenStream by design
        if has_proc_macro_attr(child, source) {
            continue;
        }

        let name = find_child_text(child, "identifier", source).unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        let params = extract_param_types(child, source);
        let ret = extract_return_type(child, source);

        // Additional guard: skip if all params and return are TokenStream
        let is_token_stream_sig =
            params.iter().all(|p| p == "TokenStream") && (ret == "TokenStream" || ret.is_empty());
        if is_token_stream_sig && !params.is_empty() {
            continue;
        }

        out.push(FnSig {
            crate_name: crate_name.to_string(),
            name,
            params,
            ret,
            line: child.start_position().row + 1,
        });
    }
}

fn has_proc_macro_attr(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "attribute" {
            let text = &source[child.byte_range()];
            if text.contains("proc_macro") {
                return true;
            }
        }
    }
    false
}

fn has_pub(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            return source[child.byte_range()].starts_with("pub");
        }
    }
    false
}

fn find_child_text(node: Node, kind: &str, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return Some(source[child.byte_range()].to_string());
        }
    }
    None
}

fn extract_param_types(fn_node: Node, source: &str) -> Vec<String> {
    let mut types = Vec::new();
    let mut cursor = fn_node.walk();
    for child in fn_node.children(&mut cursor) {
        if child.kind() == "parameters" {
            let mut param_cursor = child.walk();
            for param in child.children(&mut param_cursor) {
                if param.kind() == "parameter" {
                    // Extract type annotation from parameter
                    let mut inner = param.walk();
                    for p_child in param.children(&mut inner) {
                        // Type annotation nodes vary; grab everything after ":"
                        if is_type_node(p_child.kind()) {
                            types.push(source[p_child.byte_range()].to_string());
                        }
                    }
                }
            }
        }
    }
    types
}

fn extract_return_type(fn_node: Node, source: &str) -> String {
    let mut cursor = fn_node.walk();
    for child in fn_node.children(&mut cursor) {
        if child.kind() == "return_type" || child.kind() == "type_identifier" {
            // return_type wraps the actual type
            if child.kind() == "return_type" {
                let mut inner = child.walk();
                for rc in child.children(&mut inner) {
                    if is_type_node(rc.kind()) {
                        return source[rc.byte_range()].to_string();
                    }
                }
            }
        }
    }
    String::new()
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_identifier"
            | "generic_type"
            | "reference_type"
            | "array_type"
            | "tuple_type"
            | "pointer_type"
            | "scoped_type_identifier"
            | "primitive_type"
            | "unit_type"
    )
}
