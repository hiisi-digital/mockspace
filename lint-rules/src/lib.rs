//! AST lint rules for mockspace workspaces.
//!
//! Each lint implements the `Lint` trait (per-crate) or `CrossCrateLint` trait
//! (cross-crate). Consumers build `LintContext` for each crate and call
//! `check_crate()` / `check_cross_crate()` to run the rule sets.
//!
//! # External lint packs
//!
//! A third-party crate can ship its own lint set and be consumed by any
//! mockspace project via the `[lint-crates]` section in `mockspace.toml`.
//! The pack must expose two public functions:
//!
//! ```rust,ignore
//! pub fn lints() -> Vec<Box<dyn mockspace_lint_rules::Lint>>;
//! pub fn cross_lints() -> Vec<Box<dyn mockspace_lint_rules::CrossCrateLint>>;
//! ```
//!
//! Either may return empty. The [`lint_pack!`] macro generates both from a
//! list of lint structs. Example:
//!
//! ```rust,ignore
//! mockspace_lint_rules::lint_pack! {
//!     lints: [MyRuleA, MyRuleB],
//!     cross_lints: [MyCrossRule],
//! }
//! ```
//!
//! The generated proxy crate (`target/mockspace-proxy/`) concatenates every
//! pack's lints with any in-tree `mock/lints/*.rs` files and runs the union.

mod actionable_errors;
mod changelist_doc_gate;
pub mod changelist_helpers;
mod changelist_immutability;
mod changelist_lock;
mod changelist_required;
mod deprecation_comparison;
mod design_doc_source_mismatch;
mod export_count;
mod file_size;
mod forbidden_imports;
mod no_adhoc_error_enum;
mod no_adhoc_framework;
mod no_bare_macro_types;
mod no_bare_pub;
mod no_bare_result;
mod no_bare_string;
mod no_bare_vec;
mod no_box;
mod no_duplicate_fn;
mod no_empty_crate;
mod no_entry_suffix;
mod no_float;
mod no_manual_id;
mod no_manual_impl;
mod no_pool_access;
mod no_primitive_key;
mod no_raw_error_outside_primitives;
mod no_self_define;
mod no_todo;
mod registrable_completeness;
mod repr_c_abi_safety;
mod single_source;
pub mod type_scanner;
mod undocumented_type;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use tree_sitter::Tree;

// ---------------------------------------------------------------------------
// Proc-macro crate list (single source of truth)
// ---------------------------------------------------------------------------

/// Fallback proc-macro crate list. Empty — the caller should always pass
/// the project-specific list via `LintContext::proc_macro_crates`.
pub const PROC_MACRO_CRATES: &[&str] = &[];

// ---------------------------------------------------------------------------
// Lint trait and context
// ---------------------------------------------------------------------------

/// A single source file surfaced to lints: its repo-relative path
/// and its full text.
#[derive(Debug, Clone)]
pub struct CrateSourceFile {
    /// Crate-relative path (e.g. `src/lib.rs`, `src/bits.rs`).
    pub rel_path: std::path::PathBuf,
    /// Full file contents.
    pub text:     String,
}

/// Context provided to each lint for a single crate.
pub struct LintContext<'a> {
    /// Directory name of the crate (e.g. "<prefix>-signal").
    pub crate_name:              &'a str,
    /// Short name (e.g. "signal").
    pub short_name:              &'a str,
    /// The raw source text of `src/lib.rs` (back-compat; lints that
    /// want to scan every module file should use `all_sources`).
    pub source:                  &'a str,
    /// The tree-sitter AST of `src/lib.rs`.
    pub tree:                    &'a Tree,
    /// Every `.rs` file under the crate's `src/**`, in path order.
    /// The first entry is always `src/lib.rs`. Lints that used to
    /// inspect only `source` should iterate over this to catch drift
    /// in module files (`bits.rs`, `prim.rs`, etc.).
    pub all_sources:             &'a [CrateSourceFile],
    /// Names of all crates this crate depends on (directory names).
    pub deps:                    &'a [String],
    /// Set of all crate directory names in the workspace.
    pub all_crates:              &'a BTreeSet<String>,
    /// Content of DESIGN.md.tmpl for this crate, if it exists.
    pub design_doc:              Option<&'a str>,
    /// Concatenated content of ALL doc templates for this crate
    /// (README.md.tmpl + DESIGN.md.tmpl + DEEPDIVE_*.md.tmpl).
    pub all_doc_content:         &'a str,
    /// Content of SHAME.md.tmpl for this crate, if it exists.
    pub shame_doc:               Option<&'a str>,
    /// Root directory of the mock workspace.
    pub workspace_root:          &'a Path,
    /// Crates exempt from collection/box/float lints (proc-macro crates).
    /// Falls back to PROC_MACRO_CRATES if empty.
    pub proc_macro_crates:       &'a [String],
    /// Whether source-scanning lints should run against proc-macro crate
    /// source. Default false: skip source lints for proc-macro crates
    /// because their heap-using parsers do not ship with consumer binaries.
    /// Set true (via mockspace.toml `lint_proc_macro_source = true`) to
    /// force source lints to apply to proc-macro crates as well.
    ///
    /// Independent of expansion-based linting (future feature): what a
    /// macro emits must always satisfy consumer-crate rules because the
    /// emitted code compiles into consumer binaries.
    pub lint_proc_macro_source:  bool,
    /// Crate name prefix (e.g. "loimu"). Used to build expected crate
    /// names dynamically instead of hardcoding project-specific names.
    pub crate_prefix:            &'a str,
    /// Per-crate primitive-introductions map from mockspace.toml's
    /// `[primitive-introductions]` section. Key: crate directory
    /// name; value: list of primitive tokens the crate legitimately
    /// introduces. Lints that enforce "no bare primitives" should
    /// call [`LintContext::introduces`] to check whether the current
    /// crate legitimately uses a given primitive token.
    ///
    /// This explicit map is the belt-and-suspenders path. Long-term
    /// the introductions set should be *detected* from each crate's
    /// DESIGN.md.tmpl / source parse, not declared in a parallel
    /// TOML table. See the `Config.primitive_introductions` docs on
    /// the mockspace crate for the future direction — once that
    /// lands, the TOML map becomes additive rather than the sole
    /// source of truth.
    pub primitive_introductions: &'a BTreeMap<String, Vec<String>>,
}

impl<'a> LintContext<'a> {
    /// Whether source-scanning lints should skip this crate because it is
    /// a proc-macro crate AND the project has not opted into linting
    /// proc-macro source. This is the helper source-scanning lints should
    /// call to decide whether to short-circuit. Use this instead of
    /// [`Self::is_proc_macro_crate`] for the skip decision; that method
    /// answers the narrower question "is this a proc-macro crate?"
    /// without consulting the project's lint-behavior preference.
    #[must_use]
    pub fn should_skip_proc_macro_source_lint(&self) -> bool {
        !self.lint_proc_macro_source && self.is_proc_macro_crate()
    }

    /// Whether this crate is a proc-macro crate. Does NOT consider the
    /// `lint_proc_macro_source` preference; callers that want the
    /// "should I skip this source lint" decision should use
    /// [`Self::should_skip_proc_macro_source_lint`].
    #[must_use]
    pub fn is_proc_macro_crate(&self) -> bool {
        if self.proc_macro_crates.is_empty() {
            PROC_MACRO_CRATES.iter().any(|c| *c == self.crate_name)
        } else {
            self.proc_macro_crates.iter().any(|c| c == self.crate_name)
        }
    }

    /// Whether the current crate legitimately introduces the given
    /// primitive token per the `[primitive-introductions]` config.
    /// Lints that enforce "no bare primitives" should skip a specific
    /// token on a specific crate when this returns `true`.
    ///
    /// Example: `ctx.introduces("u8")` returns `true` when the crate
    /// is `arvo` and `arvo = ["u8", ...]` is configured in mockspace.toml.
    #[must_use]
    pub fn introduces(&self, primitive: &str) -> bool {
        self.primitive_introductions
            .get(self.crate_name)
            .map(|list| list.iter().any(|p| p == primitive))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Severity system: per-gate levels
// ---------------------------------------------------------------------------

/// What happens at a single validation gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Not reported.
    Pass,
    /// Informational note, never blocks.
    Info,
    /// Warning, never blocks but printed prominently.
    Warn,
    /// Error, blocks the gate.
    Error,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Pass => "pass",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    /// Parse a level from a string name.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "pass" | "off" => Some(Level::Pass),
            "info" => Some(Level::Info),
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

/// Which validation gate is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LintMode {
    /// Pre-commit hook. Most permissive.
    Commit,
    /// Standalone xtask run or CI. Middle strictness.
    Build,
    /// Pre-push hook. Most strict.
    Push,
}

/// Per-gate severity configuration for a lint violation.
///
/// Each lint violation declares independently what happens at each gate.
/// Use the named presets for common patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Severity {
    pub on_commit: Level,
    pub on_build:  Level,
    pub on_push:   Level,
}

impl Severity {
    /// Warns everywhere, never blocks.
    pub const ADVISORY: Self = Self::new(Level::Warn, Level::Warn, Level::Warn);
    /// Warns on commit, blocks build and push. For rules that need local
    /// iteration room but must be fixed before building.
    pub const BUILD_GATE: Self = Self::new(Level::Warn, Level::Error, Level::Error);
    /// Blocks commit, build, and push. For critical invariants.
    pub const HARD_ERROR: Self = Self::new(Level::Error, Level::Error, Level::Error);
    /// Informational only.
    pub const INFO_ONLY: Self = Self::new(Level::Info, Level::Info, Level::Info);
    /// Completely disabled — not reported at any gate.
    pub const OFF: Self = Self::new(Level::Pass, Level::Pass, Level::Pass);
    /// Warns on commit and build, blocks push only. For work-in-progress
    /// that must be clean before sharing.
    pub const PUSH_GATE: Self = Self::new(Level::Warn, Level::Warn, Level::Error);

    #[must_use]
    pub const fn new(on_commit: Level, on_build: Level, on_push: Level) -> Self {
        Self {
            on_commit,
            on_build,
            on_push,
        }
    }

    /// Whether all gates are `Level::Pass` (i.e. the lint is effectively off).
    #[must_use]
    pub fn is_off(&self) -> bool {
        self.on_commit == Level::Pass && self.on_build == Level::Pass && self.on_push == Level::Pass
    }

    /// Get the effective level for a given mode.
    #[must_use]
    pub fn effective(&self, mode: LintMode) -> Level {
        match mode {
            LintMode::Commit => self.on_commit,
            LintMode::Build => self.on_build,
            LintMode::Push => self.on_push,
        }
    }

    /// Whether this severity blocks at the given mode.
    #[must_use]
    pub fn is_blocking(&self, mode: LintMode) -> bool {
        self.effective(mode) == Level::Error
    }

    /// Human-readable label based on the gate profile.
    #[must_use]
    pub fn label(&self) -> &'static str {
        if *self == Self::OFF {
            "off"
        } else if *self == Self::HARD_ERROR {
            "error"
        } else if *self == Self::BUILD_GATE {
            "build-gate"
        } else if *self == Self::PUSH_GATE {
            "push-gate"
        } else if *self == Self::ADVISORY {
            "warn"
        } else if *self == Self::INFO_ONLY {
            "info"
        } else {
            // custom severity; label by strictest gate
            if self.on_push == Level::Error {
                "push-gate"
            } else if self.on_build == Level::Error {
                "build-gate"
            } else if self.on_commit == Level::Error {
                "error"
            } else {
                "warn"
            }
        }
    }
}

/// Whether a source line carries `lint:allow(<rule_name>)`, including
/// the case where one marker silences multiple lints at once via the
/// comma-separated form `lint:allow(rule_a, rule_b, rule_c)`. Whitespace
/// inside the parens is tolerated. Multiple `lint:allow(...)` markers on
/// the same line each get scanned.
///
/// This is the canonical allow-detection helper for every lint in this
/// crate (and downstream packs). The historical naive substring check
/// (`line.contains("lint:allow(<rule>)")`) did not handle comma-separated
/// lists; multi-lint allow comments became useless because the substring
/// never matched. Migrating every lint to this function fixes that.
#[must_use]
pub fn line_lint_allowed(line: &str, rule_name: &str) -> bool {
    let mut search = line;
    let needle = "lint:allow(";
    while let Some(start) = search.find(needle) {
        let after_open = &search[start + needle.len() ..];
        if let Some(close) = after_open.find(')') {
            let names = &after_open[.. close];
            for name in names.split(',') {
                if name.trim() == rule_name {
                    return true;
                }
            }
            search = &after_open[close ..];
        } else {
            break;
        }
    }
    false
}

/// Whether a `mod_item` node is compiled only under `cfg(test)`.
///
/// The canonical test-module predicate for this crate. A lint that walks the
/// whole tree and does not call this reports on test fixtures, which is wrong
/// for any lint whose subject is the public surface: a fixture is not API, has
/// no consumer, and cannot be documented or annotated the way the rule wants.
///
/// Two things make this harder than it looks, and every private copy of it in
/// this crate got at least one of them wrong.
///
/// The attribute is a **preceding sibling** of the module, not a child of it,
/// and the run of siblings can include doc comments, so the walk continues
/// over comments and stops only at a real item.
///
/// The predicate is **parsed rather than searched**. Asking whether the text
/// contains "cfg" and "test" answers yes for `#[cfg(not(test))]`, which is the
/// exact opposite of the question, and for `#[cfg(feature = "latest")]`,
/// because "latest" contains "test". Both would delete an ordinary module from
/// the lint's view and neither would ever be noticed, since the symptom is a
/// lint that quietly stops reporting.
///
/// Known limit: for `#[cfg(test)] mod tests;` the answer is right but does
/// nothing useful, because the module's body is a separate file that gets
/// linted on its own and never passes through this node.
#[must_use]
pub fn is_cfg_test_mod(node: tree_sitter::Node, source: &str) -> bool {
    if node.kind() != "mod_item" {
        return false;
    }
    if mod_body_has_inner_cfg_test(node, source) {
        return true;
    }
    let mut prev = node.prev_named_sibling();
    while let Some(sibling) = prev {
        match sibling.kind() {
            // Doc comments sit between an attribute and the item it applies
            // to, so they continue the run rather than ending it.
            "line_comment" | "block_comment" => {},
            "attribute_item" => {
                if attribute_is_cfg_test(&source[sibling.byte_range()]) {
                    return true;
                }
            },
            // Any real item ends the run. Without this the walk would keep
            // going back and could find a `cfg(test)` belonging to something
            // else entirely.
            _ => return false,
        }
        prev = sibling.prev_named_sibling();
    }
    false
}

/// Whether a module declares `#![cfg(test)]` as the first thing in its body.
///
/// The inner form is equivalent to the outer one and appears in real code, so
/// a predicate that only looks outside the module misses it.
fn mod_body_has_inner_cfg_test(node: tree_sitter::Node, source: &str) -> bool {
    let Some(body) = node.child_by_field_name("body") else {
        return false;
    };
    let mut cursor = body.walk();
    body.children(&mut cursor).any(|child| {
        child.kind() == "inner_attribute_item" && attribute_is_cfg_test(&source[child.byte_range()])
    })
}

/// Whether one attribute's source text is a `cfg` that holds only under test.
///
/// Takes the whole attribute including its delimiters, in either the outer
/// (`#[...]`) or inner (`#![...]`) form.
fn attribute_is_cfg_test(text: &str) -> bool {
    let text = text.trim();
    let Some(inner) = text
        .strip_prefix("#![")
        .or_else(|| text.strip_prefix("#["))
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return false;
    };
    let inner = inner.trim();
    // `cfg_attr` survives the prefix strip as `_attr(..)`, which then fails to
    // start with the open paren, so it is rejected here rather than by name.
    let Some(rest) = inner.strip_prefix("cfg") else {
        return false;
    };
    let Some(predicate) = rest
        .trim_start()
        .strip_prefix('(')
        .and_then(|p| p.strip_suffix(')'))
    else {
        return false;
    };
    cfg_predicate_is_test_only(predicate.trim())
}

/// Whether a `cfg` predicate can only hold when `test` is set.
///
/// `test` and `all(test, ..)` qualify, because both require `test`. `any(..)`
/// does not, even with `test` among its options, since the module still exists
/// in a build where one of the others is set. `not(..)` never qualifies.
fn cfg_predicate_is_test_only(predicate: &str) -> bool {
    if predicate == "test" {
        return true;
    }
    let Some(inner) = predicate
        .strip_prefix("all")
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    split_top_level(inner)
        .into_iter()
        .any(|part| cfg_predicate_is_test_only(part.trim()))
}

/// Split a comma-separated predicate list at nesting depth zero, leaving
/// commas inside nested calls and inside string literals alone.
fn split_top_level(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {},
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&input[start .. idx]);
                start = idx + ch.len_utf8();
            },
            _ => {},
        }
    }
    parts.push(&input[start ..]);
    parts
}

#[cfg(test)]
mod cfg_test_mod_tests {
    use super::*;

    /// Find the first `mod_item` anywhere in the tree and ask the predicate
    /// about it.
    fn first_mod_is_test(src: &str) -> bool {
        let mut parser = make_parser();
        let tree = parser.parse(src, None).unwrap();
        fn find<'a>(n: tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
            let mut cursor = n.walk();
            for child in n.children(&mut cursor) {
                if child.kind() == "mod_item" {
                    return Some(child);
                }
                if let Some(found) = find(child) {
                    return Some(found);
                }
            }
            None
        }
        let node = find(tree.root_node()).expect("no mod_item in fixture");
        is_cfg_test_mod(node, src)
    }

    #[test]
    fn the_attribute_is_found_as_a_preceding_sibling() {
        // The case that matters, and the one the private copies never matched:
        // tree-sitter puts the attribute beside the module, not inside it.
        assert!(first_mod_is_test("#[cfg(test)]\nmod tests { fn f() {} }"));
    }

    #[test]
    fn a_plain_module_is_not_a_test_module() {
        assert!(!first_mod_is_test("mod real { fn f() {} }"));
    }

    #[test]
    fn an_unrelated_attribute_does_not_make_a_module_a_test_module() {
        assert!(!first_mod_is_test("#[allow(dead_code)]\nmod real { fn f() {} }"));
    }

    #[test]
    fn the_walk_continues_past_a_stack_of_attributes() {
        assert!(first_mod_is_test(
            "#[cfg(test)]\n#[allow(dead_code)]\nmod tests { fn f() {} }"
        ));
    }

    #[test]
    fn the_walk_continues_past_a_comment() {
        // A doc comment between the attribute and the module is ordinary
        // formatting. Stopping on it made the predicate answer no for a module
        // that is plainly test-only.
        assert!(first_mod_is_test(
            "#[cfg(test)]\n// the unit tests\nmod tests { fn f() {} }"
        ));
        assert!(first_mod_is_test(
            "#[cfg(test)]\n/* the unit tests */\nmod tests { fn f() {} }"
        ));
    }

    #[test]
    fn a_preceding_item_that_is_not_an_attribute_stops_the_walk() {
        assert!(!first_mod_is_test(
            "#[cfg(test)]\nfn unrelated() {}\nmod real { fn f() {} }"
        ));
    }

    #[test]
    fn a_node_that_is_not_a_module_is_never_a_test_module() {
        let src = "#[cfg(test)]\nstruct S;";
        let mut parser = make_parser();
        let tree = parser.parse(src, None).unwrap();
        let mut cursor = tree.root_node().walk();
        let struct_node = tree
            .root_node()
            .children(&mut cursor)
            .find(|n| n.kind() == "struct_item")
            .expect("no struct_item in fixture");
        assert!(!is_cfg_test_mod(struct_node, src));
    }

    #[test]
    fn a_negated_test_cfg_is_not_a_test_module() {
        // `not(test)` is the exact opposite of the question. A substring search
        // for "cfg" and "test" answers yes here and deletes a module that
        // exists in every non-test build.
        assert!(!first_mod_is_test("#[cfg(not(test))]\nmod real { fn f() {} }"));
    }

    #[test]
    fn a_feature_whose_name_contains_test_is_not_a_test_module() {
        // "latest" contains "test". So does "fastest", and any feature someone
        // names later.
        assert!(!first_mod_is_test(
            "#[cfg(feature = \"latest\")]\nmod real { fn f() {} }"
        ));
    }

    #[test]
    fn a_cfg_attr_is_not_a_cfg() {
        assert!(!first_mod_is_test(
            "#[cfg_attr(test, allow(dead_code))]\nmod real { fn f() {} }"
        ));
    }

    #[test]
    fn an_all_predicate_requiring_test_is_a_test_module() {
        // `all(..)` cannot hold without every member, so a `test` anywhere in
        // it makes the module test-only.
        assert!(first_mod_is_test("#[cfg(all(test, unix))]\nmod tests { fn f() {} }"));
        assert!(first_mod_is_test("#[cfg(all(unix, test))]\nmod tests { fn f() {} }"));
    }

    #[test]
    fn an_any_predicate_offering_test_is_not_a_test_module() {
        // The module still exists in a build where the other option is set, so
        // it is public surface.
        assert!(!first_mod_is_test(
            "#[cfg(any(test, feature = \"x\"))]\nmod real { fn f() {} }"
        ));
    }

    #[test]
    fn a_comma_inside_a_string_does_not_split_the_predicate() {
        assert!(!first_mod_is_test(
            "#[cfg(all(feature = \"a,test\", unix))]\nmod real { fn f() {} }"
        ));
    }

    #[test]
    fn an_inner_attribute_marks_the_module_too() {
        // `mod tests { #![cfg(test)] .. }` is equivalent to the outer form and
        // appears in real code.
        assert!(first_mod_is_test("mod tests { #![cfg(test)]\n fn f() {} }"));
    }
}

#[cfg(test)]
mod line_lint_allowed_tests {
    use super::line_lint_allowed;

    #[test]
    fn single_name_matches() {
        assert!(line_lint_allowed(
            "use std::env; // lint:allow(no_std)",
            "no_std",
        ));
    }

    #[test]
    fn comma_list_matches_first() {
        assert!(line_lint_allowed(
            "x // lint:allow(no_std, forbidden_imports)",
            "no_std",
        ));
    }

    #[test]
    fn comma_list_matches_second() {
        assert!(line_lint_allowed(
            "x // lint:allow(forbidden_imports, no_std)",
            "no_std",
        ));
    }

    #[test]
    fn comma_list_matches_third() {
        assert!(line_lint_allowed("x // lint:allow(a, b, c)", "c"));
    }

    #[test]
    fn whitespace_tolerated() {
        assert!(line_lint_allowed("x // lint:allow(  a  ,  b  )", "a"));
        assert!(line_lint_allowed("x // lint:allow(  a  ,  b  )", "b"));
    }

    #[test]
    fn non_match_returns_false() {
        assert!(!line_lint_allowed("x // lint:allow(other)", "no_std"));
        assert!(!line_lint_allowed("x // lint:allow(a, b)", "c"));
        assert!(!line_lint_allowed("no lint marker here", "no_std"));
    }

    #[test]
    fn malformed_unclosed_returns_false() {
        assert!(!line_lint_allowed("x // lint:allow(no_std", "no_std"));
    }

    #[test]
    fn similar_name_does_not_match_partial() {
        assert!(!line_lint_allowed(
            "x // lint:allow(no_std_extra)",
            "no_std",
        ));
        assert!(!line_lint_allowed(
            "x // lint:allow(extra_no_std)",
            "no_std",
        ));
    }

    #[test]
    fn multiple_markers_on_line() {
        let line = "x // lint:allow(a) further text lint:allow(b, c)";
        assert!(line_lint_allowed(line, "a"));
        assert!(line_lint_allowed(line, "b"));
        assert!(line_lint_allowed(line, "c"));
        assert!(!line_lint_allowed(line, "d"));
    }

    #[test]
    fn empty_parens_matches_nothing() {
        assert!(!line_lint_allowed("x // lint:allow()", "anything"));
    }
}

/// Parse a severity preset from a string name.
///
/// Supports: "off", "error"/"hard-error", "build-gate", "push-gate",
/// "advisory"/"warn", "info".
#[must_use]
pub fn parse_severity(s: &str) -> Option<Severity> {
    match s.trim().to_lowercase().as_str() {
        "off" => Some(Severity::OFF),
        "error" | "hard-error" | "hard_error" => Some(Severity::HARD_ERROR),
        "build-gate" | "build_gate" => Some(Severity::BUILD_GATE),
        "push-gate" | "push_gate" => Some(Severity::PUSH_GATE),
        "advisory" | "warn" | "warning" => Some(Severity::ADVISORY),
        "info" | "info-only" | "info_only" => Some(Severity::INFO_ONLY),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Lint configuration
// ---------------------------------------------------------------------------

/// Configuration for lint severity overrides and parameters.
///
/// Parsed from the `[lints]` section of `mockspace.toml`.
#[derive(Debug, Clone)]
pub struct LintConfig {
    /// Base severity overrides per lint name.
    pub base:     HashMap<String, Severity>,
    /// Per-finding-kind severity overrides: lint_name -> { finding_kind -> severity }.
    pub findings: HashMap<String, HashMap<String, Severity>>,
    /// Per-lint parameters: lint_name -> { key -> value }.
    pub params:   HashMap<String, HashMap<String, String>>,
}

impl LintConfig {
    /// Create an empty config (all defaults).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            base:     HashMap::new(),
            findings: HashMap::new(),
            params:   HashMap::new(),
        }
    }

    /// Whether this config has any overrides at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.findings.is_empty() && self.params.is_empty()
    }

    /// Build a LintConfig from a simple base-only HashMap (backwards compat).
    pub fn from_base(base: HashMap<String, Severity>) -> Self {
        Self {
            base,
            findings: HashMap::new(),
            params: HashMap::new(),
        }
    }
}

/// A single lint violation.
#[derive(Debug, Clone)]
pub struct LintError {
    pub crate_name:   String,
    pub line:         usize,
    pub lint_name:    &'static str,
    pub message:      String,
    pub severity:     Severity,
    /// Optional sub-category for per-finding severity overrides.
    pub finding_kind: Option<&'static str>,
}

impl LintError {
    /// Create a violation that blocks all gates (commit, build, push).
    #[must_use]
    pub fn error(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity: Severity::HARD_ERROR,
            finding_kind: None,
        }
    }

    /// Create a violation that warns on commit, blocks build and push.
    #[must_use]
    pub fn build_error(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity: Severity::BUILD_GATE,
            finding_kind: None,
        }
    }

    /// Create a violation that warns on commit and build, blocks push.
    #[must_use]
    pub fn push_error(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity: Severity::PUSH_GATE,
            finding_kind: None,
        }
    }

    /// Create a warning-level violation (reported but never blocks).
    #[must_use]
    pub fn warning(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity: Severity::ADVISORY,
            finding_kind: None,
        }
    }

    /// Create an info-level violation (informational, never blocks).
    #[must_use]
    pub fn info(crate_name: String, line: usize, lint_name: &'static str, message: String) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity: Severity::INFO_ONLY,
            finding_kind: None,
        }
    }

    /// Create a violation with custom per-gate severity.
    #[must_use]
    pub fn with_severity(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
        severity: Severity,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity,
            finding_kind: None,
        }
    }

    /// Create a violation with a specific finding kind for per-finding severity overrides.
    #[must_use]
    pub fn with_finding_kind(
        crate_name: String,
        line: usize,
        lint_name: &'static str,
        message: String,
        severity: Severity,
        finding_kind: &'static str,
    ) -> Self {
        Self {
            crate_name,
            line,
            lint_name,
            message,
            severity,
            finding_kind: Some(finding_kind),
        }
    }
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(kind) = self.finding_kind {
            write!(
                f,
                "  [{lint}/{kind}] {crate_name}:{line}: [{level}] {msg}",
                lint = self.lint_name,
                kind = kind,
                crate_name = self.crate_name,
                line = self.line,
                level = self.severity.label(),
                msg = self.message,
            )
        } else {
            write!(
                f,
                "  [{lint}] {crate_name}:{line}: [{level}] {msg}",
                lint = self.lint_name,
                crate_name = self.crate_name,
                line = self.line,
                level = self.severity.label(),
                msg = self.message,
            )
        }
    }
}

/// Trait for pluggable lints. Each lint inspects a single crate's AST.
pub trait Lint {
    /// Human-readable name for error reporting.
    fn name(&self) -> &'static str;

    /// Check a crate and return any violations found.
    fn check(&self, ctx: &LintContext) -> Vec<LintError>;

    /// Whether this lint only inspects source code (not docs).
    ///
    /// Source-only lints are skipped in `--doc-only` mode (when only
    /// doc templates are staged, no `.rs` files). Default: `true`.
    fn source_only(&self) -> bool {
        true
    }

    /// The default severity for this lint's violations.
    ///
    /// Used when no config override is present. Lints should override
    /// this to match what they currently hardcode.
    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    /// Sub-categories of findings this lint can produce.
    ///
    /// Used for per-finding-kind severity overrides in config.
    fn finding_kinds(&self) -> &[&str] {
        &[]
    }

    /// Configuration keys this lint accepts.
    fn config_keys(&self) -> &[&str] {
        &[]
    }

    /// Apply configuration parameters to this lint.
    fn configure(&mut self, _params: &HashMap<String, String>) {}
}

/// Trait for cross-crate lints that compare data across all crates at once.
pub trait CrossCrateLint {
    /// Human-readable name for error reporting.
    fn name(&self) -> &'static str;

    /// Check all crates simultaneously and return any violations found.
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError>;

    /// Whether this lint only inspects source code (not docs).
    fn source_only(&self) -> bool {
        true
    }

    /// The default severity for this lint's violations.
    ///
    /// Used when no config override is present. Lints should override
    /// this to match what they currently hardcode.
    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

// ---------------------------------------------------------------------------
// External lint-pack convention
// ---------------------------------------------------------------------------

/// Declare the two `lints()` and `cross_lints()` entry points that every
/// external lint pack must expose.
///
/// Each entry is an expression producing a value that implements `Lint`
/// (in the `lints:` list) or `CrossCrateLint` (in the `cross_lints:` list).
/// Unit-struct lints are spelled `MyLint`; lints with constructors are
/// spelled `MyLint::new(args)`.
///
/// Either list may be omitted; an omitted list produces an empty `Vec`.
///
/// # Example
///
/// ```rust,ignore
/// pub struct NoBareFoo;
/// impl mockspace_lint_rules::Lint for NoBareFoo { /* ... */ }
///
/// mockspace_lint_rules::lint_pack! {
///     lints: [NoBareFoo, FileSize::new()],
/// }
/// ```
#[macro_export]
macro_rules! lint_pack {
    (
        $(lints: [ $( $lint:expr ),* $(,)? ] $(,)?)?
        $(cross_lints: [ $( $cross:expr ),* $(,)? ] $(,)?)?
    ) => {
        pub fn lints() -> ::std::vec::Vec<::std::boxed::Box<dyn $crate::Lint>> {
            #[allow(unused_mut)]
            let mut v: ::std::vec::Vec<::std::boxed::Box<dyn $crate::Lint>> = ::std::vec::Vec::new();
            $( $( v.push(::std::boxed::Box::new($lint)); )* )?
            v
        }

        pub fn cross_lints() -> ::std::vec::Vec<::std::boxed::Box<dyn $crate::CrossCrateLint>> {
            #[allow(unused_mut)]
            let mut v: ::std::vec::Vec<::std::boxed::Box<dyn $crate::CrossCrateLint>> = ::std::vec::Vec::new();
            $( $( v.push(::std::boxed::Box::new($cross)); )* )?
            v
        }
    };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all registered lint rules.
pub fn all_lints() -> Vec<Box<dyn Lint>> {
    vec![
        Box::new(no_bare_result::NoBareResult),
        Box::new(no_bare_macro_types::NoBareMacroTypes),
        Box::new(no_entry_suffix::NoEntrySuffix),
        Box::new(no_manual_impl::NoManualImpl),
        Box::new(no_adhoc_error_enum::NoAdhocErrorEnum),
        Box::new(no_manual_id::NoManualId),
        Box::new(no_primitive_key::NoPrimitiveKey),
        Box::new(no_raw_error_outside_primitives::NoRawErrorOutsidePrimitives),
        Box::new(no_pool_access::NoPoolAccess),
        Box::new(no_bare_vec::NoBareVec),
        Box::new(no_box::NoBox),
        Box::new(no_empty_crate::NoEmptyCrate),
        Box::new(design_doc_source_mismatch::DesignDocSourceMismatch),
        Box::new(actionable_errors::ActionableErrors),
        Box::new(file_size::FileSize::new()),
        Box::new(no_float::NoFloat),
        Box::new(export_count::ExportCount),
        Box::new(no_todo::NoTodo),
        Box::new(no_adhoc_framework::NoAdhocFramework),
        Box::new(no_bare_string::NoBareString),
        Box::new(no_self_define::NoSelfDefine),
        Box::new(registrable_completeness::RegistrableCompleteness),
        Box::new(repr_c_abi_safety::ReprCAbiSafety),
        Box::new(no_bare_pub::NoBarePublic),
        Box::new(forbidden_imports::ForbiddenImports::new()),
    ]
}

/// Create a tree-sitter parser configured for Rust.
pub fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set rust language");
    parser
}

/// Returns all registered cross-crate lint rules.
pub fn all_cross_crate_lints() -> Vec<Box<dyn CrossCrateLint>> {
    vec![
        Box::new(no_duplicate_fn::NoDuplicateFn),
        Box::new(single_source::SingleSource),
        Box::new(undocumented_type::UndocumentedType),
        Box::new(changelist_doc_gate::ChangelistDocGate),
        Box::new(changelist_lock::ChangelistLock),
        Box::new(changelist_required::ChangelistRequired),
        Box::new(changelist_immutability::ChangelistImmutability),
        Box::new(deprecation_comparison::DeprecationComparison),
    ]
}

/// Run all per-crate lints on a single crate, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
/// This allows doc-only commits during DOC-EXEC phase without being
/// blocked by pre-existing source issues.
///
/// When `overrides` is provided, lint severities can be overridden:
/// - If a lint name maps to a severity where all gates are `Pass`, the lint is skipped entirely.
/// - If a lint name maps to another severity, all errors from that lint use the configured severity.
/// - If a lint is not in the map, it uses its `default_severity()`.
pub fn check_crate(
    ctx: &LintContext,
    doc_only: bool,
    overrides: Option<&LintConfig>,
) -> Vec<LintError> {
    check_crate_with_extra(ctx, doc_only, overrides, &[])
}

/// Run all per-crate lints plus any custom lints on a single crate, returning violations.
pub fn check_crate_with_extra(
    ctx: &LintContext,
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn Lint>],
) -> Vec<LintError> {
    let mut lints = all_lints();

    // Configure lints with parameters from config
    if let Some(cfg) = overrides {
        for lint in &mut lints {
            if let Some(params) = cfg.params.get(lint.name()) {
                lint.configure(params);
            }
        }
    }

    let mut errors = Vec::new();

    // Helper closure to process a single lint
    let process_lint = |lint: &dyn Lint, errors: &mut Vec<LintError>| {
        if doc_only && lint.source_only() {
            return;
        }

        // A configured override wins; absent one, the lint's own declared
        // default decides whether it runs at all. Without that second half a
        // lint declaring `Severity::OFF` ran anyway, which made
        // `default_severity` decorative for every lint no consumer had named
        // in its config, and shipped opt-in lints as blocking errors.
        let base_override = overrides.and_then(|cfg| cfg.base.get(lint.name()).copied());
        if base_override.unwrap_or_else(|| lint.default_severity()).is_off() {
            return;
        }

        let mut lint_errors = lint.check(ctx);

        // Apply per-finding or base severity overrides
        if let Some(cfg) = overrides {
            for err in &mut lint_errors {
                // Check finding-specific override first
                let effective = if let (Some(kind), Some(finding_map)) =
                    (err.finding_kind, cfg.findings.get(lint.name()))
                {
                    if let Some(sev) = finding_map.get(kind) { Some(*sev) } else { base_override }
                } else {
                    base_override
                };

                if let Some(sev) = effective {
                    err.severity = sev;
                }
                // If no override, preserve the error's own severity
            }
        }

        errors.extend(lint_errors);
    };

    for lint in &lints {
        process_lint(lint.as_ref(), &mut errors);
    }
    for lint in extra_lints {
        process_lint(lint.as_ref(), &mut errors);
    }

    errors
}

/// Run all cross-crate lints, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
///
/// When `overrides` is provided, lint severities can be overridden
/// (same semantics as `check_crate`).
pub fn check_cross_crate(
    crates: &[(&str, &LintContext)],
    doc_only: bool,
    overrides: Option<&LintConfig>,
) -> Vec<LintError> {
    check_cross_crate_with_extra(crates, doc_only, overrides, &[])
}

/// Run all cross-crate lints plus any custom lints, returning violations.
pub fn check_cross_crate_with_extra(
    crates: &[(&str, &LintContext)],
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn CrossCrateLint>],
) -> Vec<LintError> {
    let lints = all_cross_crate_lints();
    let mut errors = Vec::new();

    let process_lint = |lint: &dyn CrossCrateLint, errors: &mut Vec<LintError>| {
        if doc_only && lint.source_only() {
            return;
        }

        // A configured override wins; absent one, the lint's own declared
        // default decides whether it runs at all. Without that second half a
        // lint declaring `Severity::OFF` ran anyway, which made
        // `default_severity` decorative for every lint no consumer had named
        // in its config, and shipped opt-in lints as blocking errors.
        let base_override = overrides.and_then(|cfg| cfg.base.get(lint.name()).copied());
        if base_override.unwrap_or_else(|| lint.default_severity()).is_off() {
            return;
        }

        let mut lint_errors = lint.check_all(crates);

        // Apply per-finding or base severity overrides
        if let Some(cfg) = overrides {
            for err in &mut lint_errors {
                let effective = if let (Some(kind), Some(finding_map)) =
                    (err.finding_kind, cfg.findings.get(lint.name()))
                {
                    if let Some(sev) = finding_map.get(kind) { Some(*sev) } else { base_override }
                } else {
                    base_override
                };

                if let Some(sev) = effective {
                    err.severity = sev;
                }
            }
        }

        errors.extend(lint_errors);
    };

    for lint in &lints {
        process_lint(lint.as_ref(), &mut errors);
    }
    for lint in extra_lints {
        process_lint(lint.as_ref(), &mut errors);
    }

    errors
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    struct SmokeLint;
    impl Lint for SmokeLint {
        fn name(&self) -> &'static str {
            "smoke-lint"
        }

        fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    struct SmokeCross;
    impl CrossCrateLint for SmokeCross {
        fn name(&self) -> &'static str {
            "smoke-cross"
        }

        fn check_all(&self, _crates: &[(&str, &LintContext)]) -> Vec<LintError> {
            Vec::new()
        }
    }

    mod full_pack {
        use super::{SmokeCross, SmokeLint};
        crate::lint_pack! {
            lints: [SmokeLint],
            cross_lints: [SmokeCross],
        }
    }

    mod lints_only {
        use super::SmokeLint;
        crate::lint_pack! {
            lints: [SmokeLint],
        }
    }

    mod empty_pack {
        crate::lint_pack! {}
    }

    #[test]
    fn full_pack_produces_both_vecs() {
        assert_eq!(full_pack::lints().len(), 1);
        assert_eq!(full_pack::cross_lints().len(), 1);
        assert_eq!(full_pack::lints()[0].name(), "smoke-lint");
        assert_eq!(full_pack::cross_lints()[0].name(), "smoke-cross");
    }

    #[test]
    fn lints_only_pack_empty_cross() {
        assert_eq!(lints_only::lints().len(), 1);
        assert_eq!(lints_only::cross_lints().len(), 0);
    }

    #[test]
    fn empty_pack_empty_both() {
        assert_eq!(empty_pack::lints().len(), 0);
        assert_eq!(empty_pack::cross_lints().len(), 0);
    }
}

#[cfg(test)]
mod declared_default_severity_tests {
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    use super::*;

    /// A lint that always reports once, so whether it ran is observable.
    struct AlwaysFires(&'static str, Severity);

    impl Lint for AlwaysFires {
        fn name(&self) -> &'static str {
            self.0
        }

        fn default_severity(&self) -> Severity {
            self.1
        }

        fn check(&self, ctx: &LintContext) -> Vec<LintError> {
            vec![LintError::error(ctx.crate_name.to_string(), 1, self.0, "fired".to_string())]
        }
    }

    struct AlwaysFiresAcross(&'static str, Severity);

    impl CrossCrateLint for AlwaysFiresAcross {
        fn name(&self) -> &'static str {
            self.0
        }

        fn default_severity(&self) -> Severity {
            self.1
        }

        fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
            crates
                .iter()
                .map(|(name, _)| {
                    LintError::error((*name).to_string(), 1, self.0, "fired".to_string())
                })
                .collect()
        }
    }

    fn ctx() -> LintContext<'static> {
        let mut parser = make_parser();
        let tree = parser.parse("", None).unwrap();
        LintContext {
            crate_name:              "test-crate",
            short_name:              "test-crate",
            source:                  "",
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

    fn config_of(name: &str, severity: Severity) -> LintConfig {
        let mut base = HashMap::new();
        base.insert(name.to_string(), severity);
        LintConfig { base, findings: HashMap::new(), params: HashMap::new() }
    }

    /// Count findings from one extra lint, ignoring whatever the builtin set
    /// makes of an empty crate.
    fn fired(lint: AlwaysFires, overrides: Option<&LintConfig>) -> usize {
        let name = lint.0;
        let ctx = ctx();
        check_crate_with_extra(&ctx, false, overrides, &[Box::new(lint)])
            .into_iter()
            .filter(|e| e.lint_name == name)
            .count()
    }

    fn fired_across(lint: AlwaysFiresAcross, overrides: Option<&LintConfig>) -> usize {
        let name = lint.0;
        let ctx = ctx();
        check_cross_crate_with_extra(&[("test-crate", &ctx)], false, overrides, &[Box::new(lint)])
            .into_iter()
            .filter(|e| e.lint_name == name)
            .count()
    }

    #[test]
    fn a_lint_declaring_off_does_not_run_without_a_config() {
        // The bug this fixes. `no-bare-pub` and `no-adhoc-error-enum` both
        // declare OFF because they presume machinery a project opts into, and
        // both fired as hard errors in a project that had never named them.
        assert_eq!(fired(AlwaysFires("declares-off", Severity::OFF), None), 0);
    }

    #[test]
    fn a_lint_declaring_a_real_severity_still_runs_without_a_config() {
        // The control. A resolver that skipped everything would satisfy the
        // test above.
        assert_eq!(fired(AlwaysFires("declares-error", Severity::HARD_ERROR), None), 1);
    }

    #[test]
    fn a_config_can_turn_on_a_lint_that_declares_off() {
        // Opting in is the whole point of declaring OFF rather than deleting
        // the lint, so the default must not be a floor.
        let cfg = config_of("declares-off", Severity::HARD_ERROR);
        assert_eq!(fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)), 1);
    }

    #[test]
    fn a_config_can_turn_off_a_lint_that_declares_a_real_severity() {
        // Pre-existing behaviour, kept.
        let cfg = config_of("declares-error", Severity::OFF);
        assert_eq!(fired(AlwaysFires("declares-error", Severity::HARD_ERROR), Some(&cfg)), 0);
    }

    #[test]
    fn a_config_for_another_lint_does_not_reach_this_one() {
        let cfg = config_of("some-other-lint", Severity::HARD_ERROR);
        assert_eq!(fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)), 0);
    }

    #[test]
    fn cross_crate_lints_honour_the_declared_default_too() {
        // The same resolver is written twice, so it can be fixed once and
        // still be wrong in the other half.
        assert_eq!(fired_across(AlwaysFiresAcross("declares-off", Severity::OFF), None), 0);
        assert_eq!(
            fired_across(AlwaysFiresAcross("declares-error", Severity::HARD_ERROR), None),
            1
        );
    }

    #[test]
    fn cross_crate_config_can_turn_on_a_lint_that_declares_off() {
        let cfg = config_of("declares-off", Severity::HARD_ERROR);
        assert_eq!(fired_across(AlwaysFiresAcross("declares-off", Severity::OFF), Some(&cfg)), 1);
    }
}
