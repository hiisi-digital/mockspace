//! AST lint rules for mockspace workspaces.
//!
//! Every lint implements [`Lint`], which carries what a lint has regardless of
//! its input, plus exactly one trait naming what it is handed: [`CrateLint`] for
//! one crate, [`WorkspaceLint`] for every crate at once, [`RepoLint`] for
//! repository state with no crates involved, and [`MessageLint`] for an authored
//! commit message or forge body. Consumers call `check_crate()`,
//! `check_workspace()`, `check_repo()` or `check_message()` accordingly.
//!
//! # External lint packs
//!
//! A third-party crate can ship its own lint set and be consumed by any
//! mockspace project via the `[lint-crates]` section in `mockspace.toml`.
//! The pack must expose one public function:
//!
//! ```rust,ignore
//! pub fn collect(pack: &mut mockspace_lint_rules::LintPack);
//! ```
//!
//! One entry point taking one struct, so a pack that later gains a lint kind
//! does not change the signature the host links against. The [`lint_pack!`]
//! macro generates it from lists of lint structs, any of which may be omitted:
//!
//! ```rust,ignore
//! mockspace_lint_rules::lint_pack! {
//!     lints:           [MyRuleA, MyRuleB],
//!     workspace_lints: [MyComparingRule],
//!     repo_lints:      [MyRepoStateRule],
//!     message_lints:   [MyCommitRule],
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
///
/// `Copy` so the dispatcher can hand a lint the same context with `source` and
/// `tree` swapped for one module file, which is how a per-file lint sees past
/// the crate root without every lint re-parsing files for itself.
#[derive(Clone, Copy)]
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
    /// The file within the crate the finding is in, when it is not the crate
    /// root. Carried as a field so the renderer can print a real location;
    /// a path folded into the message made `{crate}:{line}` read as a
    /// location that pointed at nothing.
    pub path:         Option<String>,
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
            path: None,
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
            path: None,
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
            path: None,
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
            path: None,
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
            path: None,
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
            path: None,
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
            path: None,
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
        // `crate/src/file.rs:line` when the finding names a file, the bare
        // `crate:line` for the crate root, so the location always points at
        // something that exists.
        let location = match &self.path {
            Some(p) => format!("{}/{}", self.crate_name, p),
            None => self.crate_name.clone(),
        };
        if let Some(kind) = self.finding_kind {
            write!(
                f,
                "  [{lint}/{kind}] {crate_name}:{line}: [{level}] {msg}",
                lint = self.lint_name,
                kind = kind,
                crate_name = location,
                line = self.line,
                level = self.severity.label(),
                msg = self.message,
            )
        } else {
            write!(
                f,
                "  [{lint}] {crate_name}:{line}: [{level}] {msg}",
                lint = self.lint_name,
                crate_name = location,
                line = self.line,
                level = self.severity.label(),
                msg = self.message,
            )
        }
    }
}

/// Trait for pluggable lints. Each lint inspects a single crate's AST.
/// What every lint has, whatever it is given.
///
/// The trait family is keyed on the *input*: [`CrateLint`] is handed one crate,
/// [`WorkspaceLint`] every crate at once, [`RepoLint`] the repository's own state
/// with no crates involved, and [`MessageLint`] an authored message. This
/// supertrait holds everything that does not depend on which of those it is.
///
/// Hoisting these was not cosmetic. `name`, `source_only` and `default_severity`
/// were previously duplicated verbatim between the crate and cross-crate traits,
/// and `finding_kinds`, `config_keys` and `configure` existed on only one of
/// them, which meant a cross-crate lint could not be configured at all.
pub trait Lint {
    /// Human-readable name for error reporting. Also the key a lint is
    /// configured under.
    fn name(&self) -> &'static str;

    /// One line on what the lint enforces, for generated documentation.
    fn description(&self) -> &'static str {
        ""
    }

    /// Whether this lint only inspects source code (not docs).
    ///
    /// Source-only lints are skipped in `--doc-only` mode (when only
    /// doc templates are staged, no `.rs` files). Default: `true`.
    fn source_only(&self) -> bool {
        true
    }

    /// Whether this lint judges one file at a time.
    ///
    /// Default `true`, which is what almost every lint here is: it reads a
    /// signature, an import or a type and has an opinion about that one file.
    /// The dispatcher runs those once per file in the crate, so a module file
    /// is checked rather than skipped.
    ///
    /// It defaults to `true` because the failure it prevents is silent. Before
    /// this existed every lint saw only `src/lib.rs`, so most of a normal
    /// crate's public surface was unlinted while the gate reported clean. A
    /// lint that opts out is visible in review; a lint that quietly checks a
    /// fraction of the crate is not.
    ///
    /// Override to `false` when the judgement is about the crate as a whole:
    /// counting its exports, comparing it against its design document, or
    /// walking `all_sources` itself. Those would either repeat their finding
    /// per file or measure a fraction of what they mean to measure.
    fn per_file(&self) -> bool {
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

    /// Whether this lint wants the invocation that triggered the run.
    ///
    /// Opt-in because most lints do not care, and because the engine only has
    /// an invocation to hand over on some paths: an agent-hook run has the
    /// triggering command, a `mock` build-gate run has none.
    fn invocation_wanted(&self) -> bool {
        false
    }
}

/// A lint handed one crate at a time.
///
/// The overwhelming majority. Was called `Lint` before the family grew, which
/// became misleading once a lint could be handed something other than a crate.
pub trait CrateLint: Lint {
    /// Check a crate and return any violations found.
    fn check(&self, ctx: &LintContext) -> Vec<LintError>;
}

/// A lint handed every crate at once, to compare them against each other.
///
/// For findings that only exist in the relation between crates: the same
/// function defined twice, one definition that must be unique across the
/// workspace, a deprecation that disagrees with its replacement.
pub trait WorkspaceLint: Lint {
    /// Check all crates simultaneously and return any violations found.
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError>;
}

/// A lint handed the repository's own state, with no crates involved.
///
/// Design-round phase, changelist locks, doc templates, config: repository
/// state that has nothing to do with any particular crate. These were
/// previously written as cross-crate lints, because that was the only hook
/// running once per workspace rather than once per crate, and they reached
/// `crates.first()` purely to steal a `workspace_root` from it. That made them
/// silently inert in a repo with **no crates at all**, since there was no crate
/// to steal from and the whole check returned no findings. This trait is handed
/// the paths directly and cannot fail that way.
pub trait RepoLint: Lint {
    /// Check repository state and return any violations found.
    ///
    /// Findings still carry a `crate_name`, derived from the offending file's
    /// own path (falling back to `"unknown"`), which is what these lints already
    /// did. There is no separate error type.
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError>;
}

/// Context for a [`RepoLint`]: the paths, and nothing crate-shaped.
pub struct RepoContext<'a> {
    /// Root directory of the mock workspace, where `design_rounds/` lives.
    pub mock_dir:      &'a Path,
    /// Root of the repository containing the mock workspace.
    pub repo_root:     &'a Path,
    /// Every crate directory name in the workspace. Empty is a legitimate
    /// state, and a repo lint must behave correctly when it is.
    pub all_crates:    &'a BTreeSet<String>,
    /// The invocation that triggered this run, when there was one and the lint
    /// asked for it via [`Lint::invocation_wanted`].
    pub invocation:    Option<Invocation<'a>>,
}

/// A lint handed an authored message rather than anything in the worktree.
///
/// A commit message and a pull-request body are authored *about* a change rather
/// than being part of it, and neither is a file the worktree contains: they
/// arrive as a message-file path, a string inside a shell command, a heredoc, or
/// a `--body-file`. A pull-request body in particular never passes through git
/// at all, so no git hook can ever see one, which is why this input kind exists.
pub trait MessageLint: Lint {
    /// Which message kinds this lint applies to. Empty means all of them.
    fn domains(&self) -> &[MessageDomain] {
        &[]
    }

    /// Check an authored message and return any violations found.
    fn check_message(&self, ctx: &MessageContext) -> Vec<LintError>;
}

/// The kind of authored message under inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDomain {
    /// A commit message, from `commit-msg`, `-m`, `-F`, or an MCP git tool.
    CommitMessage,
    /// A pull-request or merge-request body.
    PullRequestBody,
    /// An issue body or comment.
    IssueComment,
    /// A code-review comment.
    ReviewComment,
}

/// Which ruleset applies to a run: was a human in the loop, or not.
///
/// The distinction is the whole basis of attribution policy. A commit made under
/// human direction is the human's work through a tool they ran, so it carries no
/// agent byline. A commit made with no human in the chain has no other record of
/// who produced it, so provenance is wanted. Resolved from configured signals
/// rather than from one hardcoded environment variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// A human was in the loop, reading and redirecting. The default, because
    /// assuming the safer of the two is what a missing signal should mean.
    #[default]
    Assistant,
    /// Genuinely headless: no human in the chain at the time of the work.
    Autonomous,
}

impl AgentMode {
    /// The token this mode is written as in configuration and environment.
    #[must_use]
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
            Self::Autonomous => "autonomous",
        }
    }

    /// Parse a mode token, `None` when it names neither mode.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "assistant" | "assisted" | "human" => Some(Self::Assistant),
            "autonomous" | "headless" | "unattended" => Some(Self::Autonomous),
            _ => None,
        }
    }
}

/// Context for a [`MessageLint`].
pub struct MessageContext<'a> {
    /// Which kind of message this is.
    pub domain:     MessageDomain,
    /// Which ruleset applies, resolved from the configured signals.
    pub mode:       AgentMode,
    /// The authored text itself.
    pub message:    &'a str,
    /// Where the text came from, for error reporting: a file path, `-m`, a
    /// heredoc, a tool argument.
    pub origin:     &'a str,
    /// Root of the repository the message belongs to.
    pub repo_root:  &'a Path,
    /// The invocation that triggered this run, when there was one and the lint
    /// asked for it via [`Lint::invocation_wanted`].
    pub invocation: Option<Invocation<'a>>,
}

/// The command or tool call that triggered a lint run.
///
/// Absent on a plain `mock` invocation, present when a lint runs from an agent
/// hook, where the command being intercepted is the thing under inspection: a
/// `gh pr create` body lives in the command text and nowhere else.
#[derive(Debug, Clone, Copy)]
pub struct Invocation<'a> {
    /// The full shell command, heredocs and quoting intact.
    pub command:   Option<&'a str>,
    /// The tool name, for recognising MCP git and forge tools by shape.
    pub tool_name: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// External lint-pack convention
// ---------------------------------------------------------------------------

/// Declare the two `lints()` and `cross_lints()` entry points that every
/// external lint pack must expose.
///
/// Each entry is an expression producing a value that implements `Lint`
/// implementing the trait for the list it appears in.
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
        $(workspace_lints: [ $( $ws:expr ),* $(,)? ] $(,)?)?
        $(repo_lints: [ $( $repo:expr ),* $(,)? ] $(,)?)?
        $(message_lints: [ $( $msg:expr ),* $(,)? ] $(,)?)?
    ) => {
        /// Contribute this pack's lints to the host's collection.
        ///
        /// One entry point taking one struct, so a pack that gains a lint kind
        /// later does not change the signature the host links against.
        pub fn collect(#[allow(unused_variables)] pack: &mut $crate::LintPack) {
            $( $( pack.crate_lints.push(::std::boxed::Box::new($lint)); )* )?
            $( $( pack.workspace_lints.push(::std::boxed::Box::new($ws)); )* )?
            $( $( pack.repo_lints.push(::std::boxed::Box::new($repo)); )* )?
            $( $( pack.message_lints.push(::std::boxed::Box::new($msg)); )* )?
        }
    };
}

/// Every lint a pack contributes, by input kind.
///
/// The cdylib boundary passes this one value rather than a vector per kind, so
/// adding a kind is additive at the ABI rather than a signature change. Adding
/// `MessageLint` is what proved the old two-vector shape would keep breaking.
#[derive(Default)]
pub struct LintPack {
    /// Lints handed one crate at a time.
    pub crate_lints:     Vec<Box<dyn CrateLint>>,
    /// Lints handed every crate at once.
    pub workspace_lints: Vec<Box<dyn WorkspaceLint>>,
    /// Lints handed repository state, with no crates.
    pub repo_lints:      Vec<Box<dyn RepoLint>>,
    /// Lints handed an authored message.
    pub message_lints:   Vec<Box<dyn MessageLint>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all registered lint rules.
pub fn all_lints() -> Vec<Box<dyn CrateLint>> {
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
/// Run a lint over every source file in the crate, or once over the crate when
/// it declared itself crate-scoped.
///
/// This exists because `LintContext::source` is the crate root and nothing else,
/// so a lint reading it saw `src/lib.rs` and skipped every module file. In kolli
/// that was 76 of 101 public items; the gate reported clean while most of the
/// surface was never looked at. `file_size` had already been fixed by growing
/// its own loop over `all_sources`, which is the same fix twenty-six more times
/// and a fresh tree-sitter parse inside each lint.
///
/// Doing it here instead means no lint changes at all. The caller parses each
/// file once and every per-file lint shares the trees; the parse used to live
/// inside this function, which priced a crate at one parse per file per lint,
/// a loop on the wrong side of the work.
fn check_every_file(
    lint: &dyn CrateLint,
    ctx: &LintContext,
    parsed: &[(&CrateSourceFile, Tree)],
) -> Vec<LintError> {
    // An empty parse set means a caller built the context without
    // `all_sources`, so the old behaviour is what it expects rather than no
    // coverage at all.
    if !lint.per_file() || parsed.is_empty() {
        return lint.check(ctx);
    }

    let mut errors = Vec::new();
    for (file, tree) in parsed {
        let path = file.rel_path.to_string_lossy();
        let per_file = LintContext {
            source: &file.text,
            tree,
            ..*ctx
        };
        for mut err in lint.check(&per_file) {
            // The crate root stays bare, because that is where a reader
            // already looks. Anything else carries its file as a field, and
            // the renderer turns it into a location that points at something;
            // folded into the message it made `{crate}:{line}` read as a
            // location that pointed at nothing.
            if path != "src/lib.rs" {
                err.path = Some(path.to_string());
            }
            errors.push(err);
        }
    }
    errors
}

/// Parse every source file once, for the per-file dispatch to share.
///
/// A file that will not parse is dropped rather than reported; the compiler
/// says it better and says it first.
fn parse_sources(sources: &[CrateSourceFile]) -> Vec<(&CrateSourceFile, Tree)> {
    let mut parser = make_parser();
    sources
        .iter()
        .filter_map(|f| parser.parse(&f.text, None).map(|t| (f, t)))
        .collect()
}

pub fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set rust language");
    parser
}

/// Returns all registered workspace lint rules: the ones that compare crates
/// against each other.
pub fn all_workspace_lints() -> Vec<Box<dyn WorkspaceLint>> {
    vec![
        Box::new(no_duplicate_fn::NoDuplicateFn),
        Box::new(single_source::SingleSource),
        Box::new(undocumented_type::UndocumentedType),
        Box::new(deprecation_comparison::DeprecationComparison),
    ]
}

/// Returns all registered repo lint rules: the ones that inspect repository
/// state and involve no crates.
///
/// These ran as cross-crate lints until the trait family was split. They never
/// looked at a crate, so they now receive the paths directly and keep working in
/// a repo that has no crates at all.
pub fn all_repo_lints() -> Vec<Box<dyn RepoLint>> {
    vec![
        Box::new(changelist_doc_gate::ChangelistDocGate),
        Box::new(changelist_lock::ChangelistLock),
        Box::new(changelist_required::ChangelistRequired),
        Box::new(changelist_immutability::ChangelistImmutability),
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
/// - If a lint is not in the map, its `default_severity()` decides whether it
///   runs at all. It does not yet decide what its findings mean: each finding
///   keeps the severity its constructor chose, so a declared default is
///   honoured as on or off rather than per gate. See task #28.
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
    extra_lints: &[Box<dyn CrateLint>],
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

    // One parse per file, shared by every per-file lint below.
    let parsed = parse_sources(ctx.all_sources);

    for lint in lints.iter().map(Box::as_ref).chain(extra_lints.iter().map(Box::as_ref)) {
        run_with_overrides(lint, doc_only, overrides, &mut errors, || {
            check_every_file(lint, ctx, &parsed)
        });
    }
    errors
}

/// Run all cross-crate lints, returning violations.
///
/// When `doc_only` is true, skip lints that only inspect source code.
///
/// When `overrides` is provided, lint severities can be overridden
/// (same semantics as `check_crate`).
pub fn check_workspace(
    crates: &[(&str, &LintContext)],
    doc_only: bool,
    overrides: Option<&LintConfig>,
) -> Vec<LintError> {
    check_workspace_with_extra(crates, doc_only, overrides, &[])
}

/// Run all repo lints, returning violations. Involves no crates.
pub fn check_repo(
    ctx: &RepoContext,
    doc_only: bool,
    overrides: Option<&LintConfig>,
) -> Vec<LintError> {
    check_repo_with_extra(ctx, doc_only, overrides, &[])
}

/// Decide whether a lint runs, and apply the configured severity overrides to
/// whatever it found.
///
/// `run` is only called when the lint is not skipped, so a lint configured
/// `off` costs nothing. Shared by every runner below: expressible only because
/// the [`Lint`] supertrait now carries `name` and `source_only` for all of them,
/// where previously each trait declared its own copy and each runner its own
/// duplicate of this logic.
fn run_with_overrides(
    lint: &dyn Lint,
    doc_only: bool,
    overrides: Option<&LintConfig>,
    out: &mut Vec<LintError>,
    run: impl FnOnce() -> Vec<LintError>,
) {
    if doc_only && lint.source_only() {
        return;
    }

    let base_override = match overrides {
        Some(cfg) => {
            match cfg.base.get(lint.name()) {
                Some(sev) if sev.is_off() => return, // configured off: skip entirely
                Some(sev) => Some(*sev),
                None => None,
            }
        },
        None => None,
    };

    let mut lint_errors = run();

    if let Some(cfg) = overrides {
        for err in &mut lint_errors {
            let effective = if let (Some(kind), Some(finding_map)) =
                (err.finding_kind, cfg.findings.get(lint.name()))
            {
                finding_map.get(kind).copied().or(base_override)
            } else {
                base_override
            };
            if let Some(sev) = effective {
                err.severity = sev;
            }
        }
    }

    out.extend(lint_errors);
}

// FIXME: no per-file dispatch here: every workspace lint reads each crate's
// root tree only, so single-source, no-duplicate-fn and undocumented-type miss
// module files that the per-crate path now covers. A naive per-file expansion
// of the pairs corrupts no-duplicate-fn's crate-keyed suppression lookup, so
// the fix is per-lint collection with file-attributed items. tracked: #33
/// Run all workspace lints plus any custom ones, returning violations.
pub fn check_workspace_with_extra(
    crates: &[(&str, &LintContext)],
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn WorkspaceLint>],
) -> Vec<LintError> {
    let lints = all_workspace_lints();
    let mut errors = Vec::new();
    for lint in lints.iter().map(Box::as_ref).chain(extra_lints.iter().map(Box::as_ref)) {
        run_with_overrides(lint, doc_only, overrides, &mut errors, || {
            lint.check_all(crates)
        });
    }
    errors
}

/// Run all repo lints plus any custom ones, returning violations.
///
/// Takes no crates, by design. These lints inspect repository state, so they
/// must keep working in a repo whose crate list is empty.
pub fn check_repo_with_extra(
    ctx: &RepoContext,
    doc_only: bool,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn RepoLint>],
) -> Vec<LintError> {
    let lints = all_repo_lints();
    let mut errors = Vec::new();
    for lint in lints.iter().map(Box::as_ref).chain(extra_lints.iter().map(Box::as_ref)) {
        run_with_overrides(lint, doc_only, overrides, &mut errors, || {
            lint.check_repo(ctx)
        });
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
    }
    impl CrateLint for SmokeLint {
        fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    struct SmokeWorkspace;
    impl Lint for SmokeWorkspace {
        fn name(&self) -> &'static str {
            "smoke-workspace"
        }
    }
    impl WorkspaceLint for SmokeWorkspace {
        fn check_all(&self, _crates: &[(&str, &LintContext)]) -> Vec<LintError> {
            Vec::new()
        }
    }

    struct SmokeRepo;
    impl Lint for SmokeRepo {
        fn name(&self) -> &'static str {
            "smoke-repo"
        }
    }
    impl RepoLint for SmokeRepo {
        fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    struct SmokeMessage;
    impl Lint for SmokeMessage {
        fn name(&self) -> &'static str {
            "smoke-message"
        }
    }
    impl MessageLint for SmokeMessage {
        fn check_message(&self, _ctx: &MessageContext) -> Vec<LintError> {
            Vec::new()
        }
    }

    mod full_pack {
        use super::{SmokeLint, SmokeMessage, SmokeRepo, SmokeWorkspace};
        crate::lint_pack! {
            lints: [SmokeLint],
            workspace_lints: [SmokeWorkspace],
            repo_lints: [SmokeRepo],
            message_lints: [SmokeMessage],
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
    fn full_pack_contributes_every_kind() {
        let mut pack = LintPack::default();
        full_pack::collect(&mut pack);
        assert_eq!(pack.crate_lints.len(), 1);
        assert_eq!(pack.workspace_lints.len(), 1);
        assert_eq!(pack.repo_lints.len(), 1);
        assert_eq!(pack.message_lints.len(), 1);
        assert_eq!(pack.crate_lints[0].name(), "smoke-lint");
        assert_eq!(pack.workspace_lints[0].name(), "smoke-workspace");
        assert_eq!(pack.repo_lints[0].name(), "smoke-repo");
        assert_eq!(pack.message_lints[0].name(), "smoke-message");
    }

    #[test]
    fn a_pack_may_omit_every_kind_but_one() {
        let mut pack = LintPack::default();
        lints_only::collect(&mut pack);
        assert_eq!(pack.crate_lints.len(), 1);
        assert!(pack.workspace_lints.is_empty());
        assert!(pack.repo_lints.is_empty());
        assert!(pack.message_lints.is_empty());
    }

    #[test]
    fn an_empty_pack_contributes_nothing() {
        let mut pack = LintPack::default();
        empty_pack::collect(&mut pack);
        assert!(pack.crate_lints.is_empty());
        assert!(pack.workspace_lints.is_empty());
        assert!(pack.repo_lints.is_empty());
        assert!(pack.message_lints.is_empty());
    }

    /// A pack collecting into a pack that already holds lints must append rather
    /// than replace, since the host collects every pack into one value.
    #[test]
    fn collecting_two_packs_accumulates() {
        let mut pack = LintPack::default();
        full_pack::collect(&mut pack);
        lints_only::collect(&mut pack);
        assert_eq!(pack.crate_lints.len(), 2);
        assert_eq!(pack.workspace_lints.len(), 1);
    }
}

#[cfg(test)]
mod repo_lint_tests {
    use super::*;

    /// A repo lint that always reports, so the harness can observe whether it
    /// was invoked at all.
    struct AlwaysReports;

    impl Lint for AlwaysReports {
        fn name(&self) -> &'static str {
            "always-reports"
        }

        fn source_only(&self) -> bool {
            false
        }
    }

    impl RepoLint for AlwaysReports {
        fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
            vec![LintError::error(
                "unknown".to_string(),
                0,
                "always-reports",
                "invoked".to_string(),
            )]
        }
    }

    /// The bug this trait split exists to fix.
    ///
    /// Repo lints previously ran through the cross-crate hook, which handed them
    /// `&[(&str, &LintContext)]` and from which they recovered a path via
    /// `crates.first()`. With no crates that returned `None` and the lint
    /// returned no findings, so the design-round gate was silently inert in
    /// exactly the repo that most needs it: one whose taxonomy is still the
    /// subject of its first design round. A repo lint takes no crates, so it
    /// cannot regress this way.
    #[test]
    fn repo_lints_run_when_the_workspace_has_no_crates() {
        let tmp = std::env::temp_dir().join(format!("ms_repo_lint_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let no_crates = BTreeSet::new();
        let ctx = RepoContext {
            mock_dir:   &tmp,
            repo_root:  &tmp,
            all_crates: &no_crates,
            invocation: None,
        };

        let extra: Vec<Box<dyn RepoLint>> = vec![Box::new(AlwaysReports)];
        let errors = check_repo_with_extra(&ctx, false, None, &extra);
        let _ = std::fs::remove_dir_all(&tmp);

        assert_eq!(
            errors.len(),
            1,
            "a repo lint must run with an empty crate set; it received none and reported nothing"
        );
        assert_eq!(errors[0].lint_name, "always-reports");
    }

    #[test]
    fn the_changelist_family_is_registered_as_repo_lints() {
        // They only ever read the mock dir, so they belong here rather than
        // among the crate-comparing workspace lints.
        let names: Vec<&str> = all_repo_lints().iter().map(|l| l.name()).collect();
        for expected in [
            "changelist-doc-gate",
            "changelist-lock",
            "changelist-required",
            "changelist-immutability",
        ] {
            assert!(names.contains(&expected), "{expected} is not a repo lint: {names:?}");
        }
        // and none of them lingers among the workspace lints
        let ws: Vec<&str> = all_workspace_lints().iter().map(|l| l.name()).collect();
        for unexpected in ["changelist-doc-gate", "changelist-lock"] {
            assert!(!ws.contains(&unexpected), "{unexpected} still a workspace lint");
        }
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
    }

    impl CrateLint for AlwaysFires {
        fn check(&self, ctx: &LintContext) -> Vec<LintError> {
            vec![LintError::error(ctx.crate_name.to_string(), 1, self.0, "fired".to_string())]
        }
    }

    struct AlwaysFiresAcross(&'static str, Severity);

    impl Lint for AlwaysFiresAcross {
        fn name(&self) -> &'static str {
            self.0
        }

        fn default_severity(&self) -> Severity {
            self.1
        }
    }

    impl WorkspaceLint for AlwaysFiresAcross {
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

    /// A crate whose root is clean and whose module file is not.
    fn crate_with_a_dirty_module() -> Vec<CrateSourceFile> {
        vec![
            CrateSourceFile {
                rel_path: std::path::PathBuf::from("src/lib.rs"),
                text:     "pub mod env;\n".to_string(),
            },
            CrateSourceFile {
                rel_path: std::path::PathBuf::from("src/env.rs"),
                text:     "pub fn args() -> Vec<String> { todo!() }\n".to_string(),
            },
        ]
    }

    #[test]
    fn a_per_file_lint_sees_a_module_file() {
        // The bug this dispatcher exists for. `ctx.source` is the crate root and
        // nothing else, so every surface lint read `src/lib.rs` and skipped the
        // rest of the crate while the gate reported clean. In kolli that hid
        // three `Vec` in a public signature through a full review.
        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;

        let found = check_every_file(&no_bare_vec::NoBareVec, &base, &parse_sources(&files));
        assert_eq!(found.len(), 1, "expected the module file's Vec, got {found:?}");
        assert_eq!(
            found[0].path.as_deref(),
            Some("src/env.rs"),
            "a finding outside the crate root must carry its file: {found:?}",
        );
    }

    #[test]
    fn the_crate_root_alone_would_have_missed_it() {
        // The control, and the proof that the test above is measuring the
        // dispatcher rather than the lint: the same dirty fixture, dispatched
        // with only the crate root parsed, finds nothing, because the
        // violation lives in a module file the root never reads. That is
        // exactly what shipped before the dispatcher existed.
        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;

        let root_only = parse_sources(&files[.. 1]);
        let found = check_every_file(&no_bare_vec::NoBareVec, &base, &root_only);
        assert!(found.is_empty(), "the crate root is clean, so this should find nothing");
    }

    #[test]
    #[ignore = "catalogue: cross-crate lints see only crate roots; tracked #33"]
    fn a_cross_crate_lint_sees_a_module_file() {
        // The intended contract, held red on purpose: undocumented-type walks
        // every file of every crate, so an undocumented pub item in a module
        // file is found. Today the cross-crate dispatch hands each lint the
        // root tree only; the per-crate dispatcher's fix has no cross-crate
        // counterpart yet, and this flips green when it grows one.
        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;

        let found = check_workspace(&[("test-crate", &base)], false, None);
        assert!(
            found
                .iter()
                .any(|e| e.lint_name == "undocumented-type" && e.message.contains("args")),
            "an undocumented pub fn in a module file must be found: {found:?}"
        );
    }

    #[test]
    fn a_crate_scoped_lint_still_runs_once() {
        // Running one of these per file would repeat its finding per file, or
        // measure a fraction of what it means to measure. `per_file` is what
        // keeps them whole, and a lint that forgot to declare it would show up
        // here as a multiplied count.
        struct CrateScoped;
        impl Lint for CrateScoped {
            fn name(&self) -> &'static str {
                "crate-scoped"
            }

            fn per_file(&self) -> bool {
                false
            }
        }

        impl CrateLint for CrateScoped {
            fn check(&self, ctx: &LintContext) -> Vec<LintError> {
                vec![LintError::error(ctx.crate_name.to_string(), 1, "crate-scoped", "once".into())]
            }
        }

        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;
        assert_eq!(check_every_file(&CrateScoped, &base, &parse_sources(&files)).len(), 1);
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
        check_workspace_with_extra(&[("test-crate", &ctx)], false, overrides, &[Box::new(lint)])
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
    fn configuring_only_finding_severities_turns_an_off_lint_on() {
        // A consumer can write `[lints.X] findings = {..}` with no severity or
        // gate key at all. That populates `findings` and leaves `base` empty,
        // so reading only `base` would make the whole configuration inert and
        // say nothing about it.
        let mut findings = HashMap::new();
        findings.insert("declares-off".to_string(), {
            let mut kinds = HashMap::new();
            kinds.insert("some-kind".to_string(), Severity::HARD_ERROR);
            kinds
        });
        let cfg = LintConfig { base: HashMap::new(), findings, params: HashMap::new() };
        assert_eq!(fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)), 1);
    }

    #[test]
    fn configuring_only_parameters_turns_an_off_lint_on() {
        // The same shape for rule-driven lints. `forbidden-imports` is entirely
        // rules-driven and is one keystroke from this.
        let mut params = HashMap::new();
        params.insert("declares-off".to_string(), {
            let mut keys = HashMap::new();
            keys.insert("rules".to_string(), "something".to_string());
            keys
        });
        let cfg = LintConfig { base: HashMap::new(), findings: HashMap::new(), params };
        assert_eq!(fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)), 1);
    }

    #[test]
    #[ignore = "catalogue: a partially-off declared default runs at every gate; tracked #28"]
    fn a_partially_off_default_does_not_run_at_its_passing_gates() {
        // `is_off` is all or nothing, so a default of (Pass, Pass, Error) is
        // not off and the lint runs everywhere, with each finding carrying
        // whatever severity its constructor chose rather than the declared
        // per-gate shape. Asserting the intended behaviour rather than the
        // current one, so this flips to green when the gate-aware resolution
        // lands.
        let partial = Severity::new(Level::Pass, Level::Pass, Level::Error);
        assert_eq!(fired(AlwaysFires("partly-off", partial), None), 0);
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
