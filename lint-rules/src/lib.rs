//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
//! The engine compiles every pack's lints together with any in-tree
//! `mock/lints/*.rs` files into one cdylib and runs the union.

pub mod canon_not_while_panel_open;
pub mod registry_view;
pub use registry_view::{RegistryView, RowFields};
mod actionable_errors;
mod changelist_doc_gate;
pub mod changelist_helpers;
mod changelist_immutability;
mod changelist_lock;
pub(crate) mod changelist_required;
mod deprecation_comparison;
mod design_doc_source_mismatch;
mod export_count;
mod file_size;
pub mod fmt_only;
mod forbidden_imports;
pub mod merge;
mod no_adhoc_error_enum;
mod no_adhoc_framework;
mod no_bare_macro_types;
mod no_bare_pub;
mod no_duplicate_fn;
mod no_empty_crate;
mod no_entry_suffix;
mod no_manual_id;
mod no_manual_impl;
mod no_pool_access;
mod no_primitive_key;
mod no_raw_error_outside_primitives;
mod no_self_define;
mod no_todo;
pub mod path_filter;
mod registrable_completeness;
mod repr_c_abi_safety;
mod single_source;
pub mod src_layout;
pub mod tool;
pub mod type_scanner;
mod undocumented_type;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

pub use path_filter::{PathFilter, PathFilters, glob_match};
pub use tool::{
    ArgSpec,
    NotALint,
    Outcome,
    Tool,
    ToolContext,
    ToolReport,
    contract_faults,
    duplicate_tool_names,
    missing_required,
    usage_line,
};
use tree_sitter::Tree;

// ---------------------------------------------------------------------------
// Proc-macro crate list (single source of truth)
// ---------------------------------------------------------------------------

/// Fallback proc-macro crate list. Empty: the caller should always pass
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
    /// Crate name prefix (e.g. "acme"). Used to build expected crate
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
    /// the mockspace crate for the future direction: once that
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
    /// is `numerics` and `numerics = ["u8", ...]` is configured in mockspace.toml.
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
    /// Completely disabled: not reported at any gate.
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

/// True when `file` names the SHAME escape-hatch template itself.
///
/// The exemption is for a file *named* `SHAME.md.tmpl`, so the check is
/// against a whole path component. Written as a bare suffix match it also
/// exempts `NOT_SHAME.md.tmpl`, `DESIGN_SHAME.md.tmpl` and anything else
/// ending in those characters, which opens the phase gate for templates
/// nobody exempted.
///
/// Every surface that carves SHAME out of a phase gate calls this, so the
/// predicate exists once: `changelist-doc-gate`, `changelist-lock`, and the
/// generated `mockspace-write-guard` hook, whose shell regex is anchored the
/// same way.
#[must_use]
pub fn is_shame_template(file: &str) -> bool {
    file == "SHAME.md.tmpl" || file.ends_with("/SHAME.md.tmpl")
}

/// Read `<dir>/SHAME.md.tmpl`, but only when the directory really holds an
/// entry spelled exactly that.
///
/// Opening the constructed path directly is wrong on a case-insensitive
/// filesystem, which macOS uses by default: `shame.md.tmpl` opens through
/// it and its escape hatches are honoured, while both phase gates refuse
/// the file because they match case-sensitively. The same tree on Linux
/// silently stops honouring them. Listing the directory makes the reader
/// agree with the gates on every platform, and the direction is the safe
/// one: a misspelled file is inert rather than secretly authoritative.
#[must_use]
pub fn read_shame_template(dir: &std::path::Path) -> Option<String> {
    let exact = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .any(|e| e.file_name() == std::ffi::OsStr::new("SHAME.md.tmpl"));
    if !exact {
        return None;
    }
    std::fs::read_to_string(dir.join("SHAME.md.tmpl")).ok()
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
        assert!(!first_mod_is_test(
            "#[allow(dead_code)]\nmod real { fn f() {} }"
        ));
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
        assert!(!first_mod_is_test(
            "#[cfg(not(test))]\nmod real { fn f() {} }"
        ));
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
        assert!(first_mod_is_test(
            "#[cfg(all(test, unix))]\nmod tests { fn f() {} }"
        ));
        assert!(first_mod_is_test(
            "#[cfg(all(unix, test))]\nmod tests { fn f() {} }"
        ));
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
mod is_shame_template_tests {
    use super::is_shame_template;

    #[test]
    fn the_reader_honours_only_the_exact_spelling() {
        use super::read_shame_template;
        let dir = std::env::temp_dir().join(format!("shame-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // A misspelled file is inert.
        std::fs::write(dir.join("shame.md.tmpl"), "## thing\nreason\n").unwrap();
        assert_eq!(
            read_shame_template(&dir),
            None,
            "a lowercase spelling must not be honoured; the gates refuse it"
        );
        // That assertion only discriminates where the filesystem is
        // case-insensitive, because elsewhere the constructed-path form this
        // replaced also fails to open the file and the check is vacuous. Say
        // which case ran, so a green on a case-sensitive host is not read as
        // covering the divergence it was written for.
        let case_insensitive = dir.join("SHAME.md.tmpl").is_file();
        if !case_insensitive {
            eprintln!(
                "note: this filesystem is case-sensitive, so the case half of \
                 read_shame_template is not exercised here; it is exercised on \
                 macOS and on any case-insensitive volume"
            );
        }

        std::fs::remove_file(dir.join("shame.md.tmpl")).unwrap();
        std::fs::write(dir.join("SHAME.md.tmpl"), "## thing\nreason\n").unwrap();
        assert_eq!(
            read_shame_template(&dir).as_deref(),
            Some("## thing\nreason\n")
        );

        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            read_shame_template(&dir),
            None,
            "a missing dir reads as absent"
        );
    }

    #[test]
    fn only_a_whole_path_component_named_shame_matches() {
        assert!(is_shame_template("crates/foo/SHAME.md.tmpl"));
        assert!(is_shame_template("crates/SHAME.md.tmpl"));
        // The bare-filename arm. No caller reaches it today, since all
        // three require a `crates/` prefix first, so nothing else can
        // exercise it.
        assert!(is_shame_template("SHAME.md.tmpl"));

        assert!(!is_shame_template("crates/foo/NOT_SHAME.md.tmpl"));
        assert!(!is_shame_template("crates/foo/DESIGN_SHAME.md.tmpl"));
        assert!(!is_shame_template("crates/foo/SHAME.md.tmpl.bak"));
        assert!(!is_shame_template("crates/SHAME.md.tmpl/inner.md.tmpl"));
        assert!(!is_shame_template(""));
        // Matching is case-sensitive, deliberately: the escape hatch is
        // one exact filename. A case-insensitive filesystem will let a
        // consumer create `shame.md.tmpl` and find it gated, which is
        // the safe direction; the reverse would silently open the gate.
        assert!(!is_shame_template("crates/foo/shame.md.tmpl"));
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
    /// Which paths each lint may be shown: lint_name -> filter.
    ///
    /// The runner applies these before calling a lint, so no lint implements path scoping
    /// and one written without knowing this exists still respects it. See [`path_filter`].
    pub paths:    PathFilters,
}

impl LintConfig {
    /// Create an empty config (all defaults).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            base:     HashMap::new(),
            findings: HashMap::new(),
            params:   HashMap::new(),
            paths:    PathFilters::new(),
        }
    }

    /// Whether this config has any overrides at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
            && self.findings.is_empty()
            && self.params.is_empty()
            && self.paths.is_empty()
    }

    /// The path filter for one lint, when it has one that would change anything. An empty
    /// filter reads as absent, so the runner skips the work instead of filtering a list
    /// down to itself.
    #[must_use]
    pub fn filter_for(&self, lint_name: &str) -> Option<&PathFilter> {
        self.paths.get(lint_name).filter(|f| !f.is_empty())
    }

    /// Build a LintConfig from a simple base-only HashMap (backwards compat).
    pub fn from_base(base: HashMap<String, Severity>) -> Self {
        Self {
            base,
            findings: HashMap::new(),
            params: HashMap::new(),
            paths: PathFilters::new(),
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

    /// The default severity for this lint's violations, when no config
    /// override is present.
    ///
    /// **Every lint declares its own.** The default here exists so the trait
    /// can be implemented, and a lint that reaches it has forgotten to say
    /// what it is; a test enumerates the registry and refuses that.
    ///
    /// It is off rather than blocking, because the two failures are not
    /// symmetric. A lint that should gate and is silent is caught by the thing
    /// it was meant to catch getting through. A lint that should be off and
    /// blocks refuses a stranger's first commit and names a macro their project
    /// does not have, which is not a state they can reason their way out of.
    /// Three lints encoding one downstream project's architecture shipped that
    /// way, inheriting a blocking default nobody had written down.
    fn default_severity(&self) -> Severity {
        Severity::OFF
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
    pub mock_dir:    &'a Path,
    /// Root of the repository containing the mock workspace.
    pub repo_root:   &'a Path,
    /// Every crate directory name in the workspace. Empty is a legitimate
    /// state, and a repo lint must behave correctly when it is.
    pub all_crates:  &'a BTreeSet<String>,
    /// Every directory holding source packages, absolute, in config order.
    ///
    /// The same list `mockspace.toml` names and `Config::src_dirs` carries, in
    /// the same form, so there is one spelling of it rather than an absolute
    /// one out here and a relative one in the lints. A lint wanting a git
    /// pathspec strips `mock_dir` off itself.
    ///
    /// Empty is legitimate and means a project with no source at all, which is
    /// what a documentation repository is.
    pub src_dirs:    &'a [PathBuf],
    /// The invocation that triggered this run, when there was one and the lint
    /// asked for it via [`Lint::invocation_wanted`].
    pub invocation:  Option<Invocation<'a>>,
    /// The globs a project declares as its canon, from `canon_paths`. Empty
    /// where a project has not said what its canon is, which is most of them.
    pub canon_paths: &'a [String],
    /// The slugs of every panel presently open, computed by the engine because
    /// the panel ledger lives there. Empty where none is.
    pub open_panels: &'a [String],
    /// The project's registry, flattened, with the reverse edges computed.
    ///
    /// Empty where the project declares no registry, which is legitimate and
    /// which every accessor answers without a special case.
    pub registry:    &'a crate::RegistryView,
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
        $(tools: [ $( $tool:expr ),* $(,)? ] $(,)?)?
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
            $( $( pack.tools.push(::std::boxed::Box::new($tool)); )* )?
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
    /// Tools: checks invoked as `mock <name>` because they cannot be lints.
    ///
    /// Not lints, and carried here anyway, because they arrive through the same
    /// cdylib and the boundary passes one value. Splitting them into a second
    /// pack would mean a second collector symbol, a second dlopen, and two
    /// lifetimes to keep straight, to express a distinction the `Tool` trait
    /// already makes.
    pub tools:           Vec<Box<dyn tool::Tool>>,
}

/// Every lint name registered more than once across the builtin sets and a
/// pack, sorted.
///
/// A name registered twice runs twice and reports twice, and a
/// `[lints.<name>]` entry is ambiguous between the two, so at least one
/// registration is unconfigurable. That state shipped: the builtin set and
/// the stack pack both registered `no-bare-result` and `no-bare-string` for
/// months, and a pack's own uniqueness test cannot see across the boundary.
/// The host checks this before running anything and treats a hit as a hard
/// configuration error rather than degrading into doubled findings.
#[must_use]
pub fn duplicate_lint_names(pack: &LintPack) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut bump = |name: &str| *counts.entry(name.to_string()).or_insert(0) += 1;

    for l in all_lints() {
        bump(l.name());
    }
    for l in all_workspace_lints() {
        bump(l.name());
    }
    for l in all_repo_lints() {
        bump(l.name());
    }
    for l in &pack.crate_lints {
        bump(l.name());
    }
    for l in &pack.workspace_lints {
        bump(l.name());
    }
    for l in &pack.repo_lints {
        bump(l.name());
    }
    for l in &pack.message_lints {
        bump(l.name());
    }

    counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(name, _)| name)
        .collect()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all registered lint rules.
pub fn all_lints() -> Vec<Box<dyn CrateLint>> {
    vec![
        Box::new(no_bare_macro_types::NoBareMacroTypes),
        Box::new(no_entry_suffix::NoEntrySuffix),
        Box::new(no_manual_impl::NoManualImpl),
        Box::new(no_adhoc_error_enum::NoAdhocErrorEnum),
        Box::new(no_manual_id::NoManualId),
        Box::new(no_primitive_key::NoPrimitiveKey),
        Box::new(no_raw_error_outside_primitives::NoRawErrorOutsidePrimitives),
        Box::new(no_pool_access::NoPoolAccess),
        Box::new(no_empty_crate::NoEmptyCrate),
        Box::new(design_doc_source_mismatch::DesignDocSourceMismatch),
        Box::new(actionable_errors::ActionableErrors),
        Box::new(file_size::FileSize::new()),
        Box::new(export_count::ExportCount),
        Box::new(no_todo::NoTodo),
        Box::new(no_adhoc_framework::NoAdhocFramework),
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
/// so a lint reading it saw `src/lib.rs` and skipped every module file. In one crate
/// that was 76 of 101 public items; the gate reported clean while most of the
/// surface was never looked at. `file_size` had already been fixed by growing
/// its own loop over `all_sources`, which is the same fix twenty-six more times
/// and a fresh tree-sitter parse inside each lint.
///
/// Doing it here instead means no lint changes at all. The caller parses each
/// file once and every per-file lint shares the trees; the parse used to live
/// inside this function, which priced a crate at one parse per file per lint,
/// a loop on the wrong side of the work.
/// # Where the path filter gets applied, and why to both dispatch shapes
///
/// The filter has to reach the crate-scoped case as well as the per-file one, or it would
/// be a rule any lint could step around by declaring `per_file = false` and walking
/// `all_sources` for itself. So a filtered crate-scoped lint gets a context whose
/// `all_sources` holds only what it may see, and whose `source` and `tree` point at the
/// first surviving file rather than at a crate root it was configured not to look at.
///
/// That costs a clone of each surviving file, paid only by a lint someone actually
/// configured a filter for: [`LintConfig::filter_for`] hands back `None` for every other
/// lint and this takes the borrowed path.
///
/// A lint whose filter admits nothing is skipped rather than run against an empty crate,
/// since "nothing to look at" and "looked and found nothing" are different answers and
/// something like `no-empty-crate` reports on the second.
fn check_every_file(
    lint: &dyn CrateLint,
    ctx: &LintContext,
    parsed: &[(&CrateSourceFile, Tree)],
    filter: Option<&PathFilter>,
) -> Vec<LintError> {
    let kept: Vec<&(&CrateSourceFile, Tree)> = match filter {
        None => parsed.iter().collect(),
        Some(f) => {
            parsed
                .iter()
                .filter(|(file, _)| f.allows(&file.rel_path))
                .collect()
        },
    };

    // `parsed` and `ctx.all_sources` describe the same files for every real
    // caller, since `check_crate_with_extra` derives the first from the second.
    // Where they could disagree, `kept` is the one the crate-scoped branch below
    // reads a root from and `owned` is the one it hands over, so an empty
    // `parsed` against a non-empty `all_sources` would index nothing. Returning
    // here rather than falling through keeps that unreachable rather than
    // relying on the caller to keep it so.
    if filter.is_some() && kept.is_empty() {
        return Vec::new();
    }

    if !lint.per_file() {
        // an unfiltered crate-scoped lint gets the context untouched, which is the path
        // every lint took before filters existed
        let Some(f) = filter else {
            return lint.check(ctx);
        };
        let owned: Vec<CrateSourceFile> = ctx
            .all_sources
            .iter()
            .filter(|s| f.allows(&s.rel_path))
            .cloned()
            .collect();
        if owned.is_empty() {
            return Vec::new();
        }
        let (root_source, root_tree) = (&kept[0].0.text, &kept[0].1);
        return lint.check(&LintContext {
            source: root_source,
            tree: root_tree,
            all_sources: &owned,
            ..*ctx
        });
    }

    // An empty parse set means a caller built the context without
    // `all_sources`, so the old behaviour is what it expects rather than no
    // coverage at all.
    if parsed.is_empty() {
        return lint.check(ctx);
    }

    let mut errors = Vec::new();
    for (file, tree) in kept {
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
        Box::new(canon_not_while_panel_open::CanonNotWhilePanelOpen),
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
/// - If a lint name maps to a severity where all gates are `Pass`, the lint
///   is skipped entirely.
/// - If a lint name maps to another severity, all errors from that lint use
///   the configured severity.
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

    for lint in lints
        .iter()
        .map(Box::as_ref)
        .chain(extra_lints.iter().map(Box::as_ref))
    {
        let filter = overrides.and_then(|cfg| cfg.filter_for(lint.name()));
        run_with_overrides(lint, doc_only, overrides, &mut errors, || {
            check_every_file(lint, ctx, &parsed, filter)
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
/// `run` is only called when the lint is not skipped, so a lint that is off
/// costs nothing. Shared by every runner below: expressible only because
/// the [`Lint`] supertrait now carries `name` and `source_only` for all of them,
/// where previously each trait declared its own copy and each runner its own
/// duplicate of this logic.
///
/// Enablement resolves config first, then the lint's own declared default. That
/// second half was missing and made [`Lint::default_severity`] decorative for
/// the `off` case: fourteen project-specific lints declared `OFF`, stamped
/// `HARD_ERROR` onto their findings anyway, and fired as hard errors in every
/// repo that had not explicitly overridden them. A fresh repo with no `[lints]`
/// section therefore inherited a framework's house rules (`define_error!`,
/// `#[public_api]`, `Collection<T>`) it had never opted into.
///
/// Only an explicit override restamps a finding's severity. A lint's own
/// per-finding levels are deliberate gradation, a build gate and an advisory
/// coming out of one lint, and flattening them onto the lint's default would
/// erase it.
/// The lints a project may not turn off or lower.
///
/// The design-round gate is what this tool is. A project that wants source
/// edits ungated does not want this tool, and the readme says so in those
/// words, so the sentence has to be true rather than a description of what
/// nobody had tried. Configuring one of these parses and then does nothing,
/// which is quieter than it should be; saying so to the person who wrote the
/// line wants a diagnostic channel the config reader does not have yet.
///
/// Held here by name rather than declared by each lint, because a lint that
/// declares itself non-negotiable is a lint pack deciding what a project it
/// was imported into must accept. That is the opposite of the arrangement.
pub const NON_NEGOTIABLE: [&str; 4] = [
    "changelist-required",
    "changelist-doc-gate",
    "changelist-lock",
    "changelist-immutability",
];

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

    // Read past the config entirely for the four the readme calls
    // non-negotiable, whether it arrived from a file or was built in code.
    let negotiable = !NON_NEGOTIABLE.contains(&lint.name());
    let base_override = overrides
        .filter(|_| negotiable)
        .and_then(|cfg| cfg.base.get(lint.name()).copied());

    // A configured override wins; absent one, the lint's own declared default
    // decides whether it runs at all. Naming a lint in any section counts as
    // asking for it: a consumer can configure only `findings` or only
    // `params`, which populates neither `base` nor anything else the override
    // lookup sees, and reading that as silence would make the configuration
    // inert without saying so.
    let named_in_config = overrides.is_some_and(|cfg| {
        cfg.base.contains_key(lint.name())
            || cfg.findings.contains_key(lint.name())
            || cfg.params.contains_key(lint.name())
            || cfg.paths.contains_key(lint.name())
    });
    if negotiable && !named_in_config && lint.default_severity().is_off() {
        return;
    }
    if base_override.is_some_and(|sev| sev.is_off()) {
        return;
    }

    let mut lint_errors = run();

    if let Some(cfg) = overrides.filter(|_| negotiable) {
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
    for lint in lints
        .iter()
        .map(Box::as_ref)
        .chain(extra_lints.iter().map(Box::as_ref))
    {
        run_with_overrides(lint, doc_only, overrides, &mut errors, || {
            lint.check_all(crates)
        });
    }
    errors
}

/// Run all message lints plus any custom ones, returning violations.
///
/// A lint declaring domains runs only for those; declaring none runs for all, so
/// an attribution rule covers every surface without enumerating them.
///
/// mockspace ships no message lints of its own: a commit convention is not
/// something an engine should impose, so every one arrives from a pack the project
/// chose to import.
pub fn check_message_with_extra(
    ctx: &MessageContext,
    overrides: Option<&LintConfig>,
    extra_lints: &[Box<dyn MessageLint>],
) -> Vec<LintError> {
    let mut errors = Vec::new();
    for lint in extra_lints.iter().map(Box::as_ref) {
        let domains = lint.domains();
        if !domains.is_empty() && !domains.contains(&ctx.domain) {
            continue;
        }
        // `doc_only` is false: a message is never a doc template, so the
        // source-only skip does not apply to this domain.
        run_with_overrides(lint, false, overrides, &mut errors, || {
            lint.check_message(ctx)
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
    for lint in lints
        .iter()
        .map(Box::as_ref)
        .chain(extra_lints.iter().map(Box::as_ref))
    {
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

        /// Declared rather than inherited, which is what makes this fixture
        /// the permit half of the pair: the trait's own default is off, so a
        /// fixture that says nothing is the same fixture as the off one.
        fn default_severity(&self) -> Severity {
            Severity::HARD_ERROR
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
        let no_src: [PathBuf; 0] = [];
        let ctx = RepoContext {
            mock_dir:    &tmp,
            repo_root:   &tmp,
            all_crates:  &no_crates,
            src_dirs:    &no_src,
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            registry:    &Default::default(),
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
    fn every_registered_lint_says_what_its_severity_is() {
        // The trait carries a default so it can be implemented, and a lint
        // reaching it has not decided. That used to be invisible and the
        // default used to block: three lints encoding one downstream project's
        // architecture inherited a hard error and refused a stranger's first
        // commit, naming macros and traits their project does not have.
        //
        // Reading the source rather than calling `default_severity`, because
        // the value a lint returns is the same whether it declared it or
        // inherited it. Declaring is what is under test.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let declared: std::collections::HashSet<String> = std::fs::read_dir(&dir)
            .expect("no source directory")
            .filter_map(|e| {
                let path = e.ok()?.path();
                let text = std::fs::read_to_string(&path).ok()?;
                text.contains("fn default_severity(&self)")
                    .then(|| path.file_stem()?.to_str().map(str::to_string))
                    .flatten()
            })
            .collect();

        // The control. A predicate that matched nothing would report every
        // lint undeclared, and one that matched everything would report none.
        assert!(
            declared.contains("no_todo"),
            "the reader found nothing: it did not see a file that plainly declares one"
        );
        assert!(
            !declared.contains("lints_are_not_a_module"),
            "the reader answered for a file that does not exist"
        );

        let registered: Vec<String> = all_lints()
            .iter()
            .map(|l| l.name().replace('-', "_"))
            .chain(all_repo_lints().iter().map(|l| l.name().replace('-', "_")))
            .collect();

        // The lint's name and its module's name agree everywhere today, and
        // that is what makes this readable at all. A rename that breaks the
        // correspondence shows up here as an undeclared lint rather than
        // silently stopping checking anything.
        let missing: Vec<&String> = registered
            .iter()
            .filter(|name| !declared.contains(*name))
            .collect();
        assert!(
            missing.is_empty(),
            "these registered lints inherit the trait's default instead of \
             declaring one: {missing:?}"
        );
    }

    /// A lint that is off unless a repo asks for it, and that stamps a hard
    /// error onto its finding. The combination is not contrived: fourteen
    /// registered lints have exactly this shape.
    struct OffByDefault;

    impl Lint for OffByDefault {
        fn name(&self) -> &'static str {
            "off-by-default"
        }

        fn default_severity(&self) -> Severity {
            Severity::OFF
        }

        fn source_only(&self) -> bool {
            false
        }
    }

    impl RepoLint for OffByDefault {
        fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
            vec![LintError::error(
                "unknown".to_string(),
                0,
                "off-by-default",
                "invoked".to_string(),
            )]
        }
    }

    /// Run one repo lint against a throwaway directory.
    fn run_one(lint: Box<dyn RepoLint>, cfg: Option<&LintConfig>) -> Vec<LintError> {
        let tmp = std::env::temp_dir().join(format!(
            "ms_sev_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let no_crates = BTreeSet::new();
        let no_src: [PathBuf; 0] = [];
        let ctx = RepoContext {
            mock_dir:    &tmp,
            repo_root:   &tmp,
            all_crates:  &no_crates,
            src_dirs:    &no_src,
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            registry:    &Default::default(),
        };
        let extra: Vec<Box<dyn RepoLint>> = vec![lint];
        let errors = check_repo_with_extra(&ctx, false, cfg, &extra);
        let _ = std::fs::remove_dir_all(&tmp);
        errors
    }

    /// Naming a lint in any section counts as asking for it, and `paths` is a
    /// section. It was added without being added here, so a lint that is off
    /// until asked and is asked with nothing but a path filter stayed off,
    /// which is the same defect this enumeration already exists to prevent one
    /// section over.
    #[test]
    fn a_path_filter_alone_is_enough_to_ask_for_an_off_by_default_lint() {
        let mut cfg = LintConfig::empty();
        cfg.paths.insert("off-by-default".to_string(), PathFilter {
            include: vec!["**/*.rs".to_string()],
            exclude: Vec::new(),
        });
        let errors = run_one(Box::new(OffByDefault), Some(&cfg));
        assert_eq!(
            errors.len(),
            1,
            "a lint named only under [lints.<name>] include/exclude must run; \
             it did not, so the filter section is configuration nothing reads"
        );
    }

    /// The control. Nothing in the config at all leaves it off, so the test
    /// above is measuring the filter section rather than measuring that this
    /// lint runs no matter what.
    #[test]
    fn an_unmentioned_off_by_default_lint_stays_off() {
        assert!(run_one(Box::new(OffByDefault), None).is_empty());
        assert!(run_one(Box::new(OffByDefault), Some(&LintConfig::empty())).is_empty());
    }

    /// Catalogued. `include` and `exclude` under a repo lint's name parse, are
    /// carried into `LintConfig`, and are then consulted by nobody, because
    /// `check_repo_with_extra` hands the lint a context and no file list. The
    /// assertion is what it should do; un-ignore it when the filter reaches
    /// repo lints. `path_filter`'s module doc names the same gap.
    #[test]
    #[ignore = "catalogue: include/exclude under a repo lint's name are inert, \
                since the repo dispatch consults no filter"]
    fn a_repo_lint_excluded_from_everything_does_not_run() {
        let mut cfg = LintConfig::empty();
        cfg.paths.insert("off-by-default".to_string(), PathFilter {
            include: Vec::new(),
            exclude: vec!["**".to_string()],
        });
        assert!(run_one(Box::new(OffByDefault), Some(&cfg)).is_empty());
    }

    #[test]
    fn the_design_round_gate_cannot_be_turned_off() {
        // The readme calls these four always on and non-negotiable, and the
        // whole pitch rests on the sentence. Nothing enforced it: one line in
        // a `[lints]` table disarmed the gate the tool exists to be, and the
        // repository it disarmed was the one whose invariants it was holding.
        for name in NON_NEGOTIABLE {
            let mut cfg = LintConfig::empty();
            cfg.base.insert(name.to_string(), Severity::OFF);

            let errors = run_one(Box::new(Ungovernable(name)), Some(&cfg));
            assert_eq!(errors.len(), 1, "`{name}` was turned off by a config line");
            assert_eq!(
                errors[0].severity,
                Severity::HARD_ERROR,
                "`{name}` was lowered by a config line"
            );
        }
    }

    #[test]
    fn an_ordinary_lint_is_still_the_project_s_to_configure() {
        // The control, and the reason the list is a list rather than a policy.
        // A guard written as "no override ever wins" passes the test above and
        // takes every other lint's configuration with it.
        let mut cfg = LintConfig::empty();
        cfg.base.insert("negotiable".to_string(), Severity::OFF);
        assert!(
            run_one(Box::new(Ungovernable("negotiable")), Some(&cfg)).is_empty(),
            "an ordinary lint ignored the project's own configuration"
        );
    }

    /// A repo lint that always reports, under whatever name it is given.
    ///
    /// Named rather than fixed, so the same fixture stands in for each of the
    /// four and for one that is not on the list. What is under test is the
    /// name, and a fixture per name would test four copies of one branch.
    struct Ungovernable(&'static str);

    impl Lint for Ungovernable {
        fn name(&self) -> &'static str {
            self.0
        }

        fn default_severity(&self) -> Severity {
            Severity::HARD_ERROR
        }

        fn source_only(&self) -> bool {
            false
        }
    }

    impl RepoLint for Ungovernable {
        fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
            vec![LintError::error("unknown".to_string(), 0, self.0, "fired".to_string())]
        }
    }

    #[test]
    fn a_lint_that_declares_itself_off_does_not_run() {
        // Severity resolution consulted config and never the lint's own default,
        // so `default_severity() == OFF` meant nothing and the lint fired at
        // whatever level its findings stamped. That is how a fresh repo with no
        // `[lints]` section inherited a framework's house rules.
        let errors = run_one(Box::new(OffByDefault), None);
        assert!(
            errors.is_empty(),
            "an off-by-default lint ran anyway and reported {errors:?}"
        );
    }

    #[test]
    fn a_lint_that_declares_a_real_default_still_runs() {
        // The permit path. Without this, a resolution bug that skips every lint
        // passes the test above and disarms the whole gate silently.
        let errors = run_one(Box::new(AlwaysReports), None);
        assert_eq!(errors.len(), 1, "an on-by-default lint must still run");
        assert_eq!(errors[0].severity, Severity::HARD_ERROR);
    }

    #[test]
    fn a_repo_can_opt_into_a_lint_that_is_off_by_default() {
        // Off by default is not off for good: a repo that does follow the
        // convention names the lint and gets it.
        let mut base = HashMap::new();
        base.insert("off-by-default".to_string(), Severity::HARD_ERROR);
        let cfg = LintConfig::from_base(base);
        let errors = run_one(Box::new(OffByDefault), Some(&cfg));
        assert_eq!(errors.len(), 1, "an opted-into lint must run");
        assert_eq!(errors[0].severity, Severity::HARD_ERROR);
    }

    #[test]
    fn an_off_override_silences_a_lint_that_is_on_by_default() {
        let mut base = HashMap::new();
        base.insert("always-reports".to_string(), Severity::OFF);
        let cfg = LintConfig::from_base(base);
        let errors = run_one(Box::new(AlwaysReports), Some(&cfg));
        assert!(errors.is_empty(), "an off override must silence the lint");
    }

    #[test]
    fn a_findings_own_severity_survives_when_no_override_names_the_lint() {
        // A lint may grade its own findings, a build gate here and an advisory
        // there. Resolution must not flatten that onto the lint's default.
        let cfg = LintConfig::empty();
        let errors = run_one(Box::new(AlwaysReports), Some(&cfg));
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].severity,
            Severity::HARD_ERROR,
            "the finding's own level was overwritten by the lint's default"
        );
    }

    #[test]
    fn an_explicit_base_override_restamps_the_finding() {
        let mut base = HashMap::new();
        base.insert("always-reports".to_string(), Severity::ADVISORY);
        let cfg = LintConfig::from_base(base);
        let errors = run_one(Box::new(AlwaysReports), Some(&cfg));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::ADVISORY);
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
            assert!(
                names.contains(&expected),
                "{expected} is not a repo lint: {names:?}"
            );
        }
        // and none of them lingers among the workspace lints
        let ws: Vec<&str> = all_workspace_lints().iter().map(|l| l.name()).collect();
        for unexpected in ["changelist-doc-gate", "changelist-lock"] {
            assert!(
                !ws.contains(&unexpected),
                "{unexpected} still a workspace lint"
            );
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

    /// Ordinary Rust, using names three registered lints happen to know.
    ///
    /// `Action` and `Scope` are traits any project might declare. `define_*!`
    /// is a macro-naming convention any project might follow. None of it is a
    /// reference to the framework whose house rules those lints encode, and a
    /// stranger writing it has no way to know their commit is about to be
    /// refused and told to use a macro they have never heard of.
    fn a_stranger_s_ordinary_crate() -> Vec<CrateSourceFile> {
        vec![CrateSourceFile {
            rel_path: std::path::PathBuf::from("src/lib.rs"),
            text:     "pub trait Action { fn run(&self); }\n\
                       pub trait Scope { fn name(&self) -> &str; }\n\
                       pub struct Jump;\n\
                       impl Action for Jump { fn run(&self) {} }\n\
                       pub struct Local;\n\
                       impl Scope for Local { fn name(&self) -> &str { \"local\" } }\n\
                       #[macro_export]\n\
                       macro_rules! define_thing { () => {} }\n"
                .to_string(),
        }]
    }

    #[test]
    fn a_stranger_s_first_commit_is_not_refused_by_somebody_else_s_house_rules() {
        // Thirteen registered lints encode one downstream project's
        // architecture with identifier tables nobody else can change. Ten were
        // given `Severity::OFF` and three were not, so they inherited the
        // trait's default, which was a hard error, and refused a fresh repo's
        // first commit naming macros and traits that project does not have.
        let files = a_stranger_s_ordinary_crate();
        let mut parser = make_parser();
        let tree = parser
            .parse(&files[0].text, None)
            .expect("the fixture does not parse");
        let mut base = ctx();
        base.source = &files[0].text;
        base.tree = &tree;
        base.all_sources = &files;

        let found = check_crate(&base, false, None);
        let blocking: Vec<&LintError> = found
            .iter()
            .filter(|e| e.severity.effective(LintMode::Commit) != Level::Pass)
            .collect();
        assert!(
            blocking.is_empty(),
            "a repo with no [lints] section is refused: {:?}",
            blocking
                .iter()
                .map(|e| (e.lint_name, &e.message))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_check_above_can_fail() {
        // The control on it, and the one that matters: a `check_crate` that
        // found nothing at all, or a severity reader that called everything
        // pass, would report the same empty list for a repo the gate should
        // refuse. The fixture below carries a `todo!()`, which `no-todo`
        // gates at push.
        let files = crate_with_a_dirty_module();
        let mut parser = make_parser();
        let tree = parser
            .parse(&files[0].text, None)
            .expect("the fixture does not parse");
        let mut base = ctx();
        base.source = &files[0].text;
        base.tree = &tree;
        base.all_sources = &files;

        let found = check_crate(&base, false, None);
        assert!(
            found
                .iter()
                .any(|e| e.severity.effective(LintMode::Push) != Level::Pass),
            "the reader reports every finding as a pass: {found:?}"
        );
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
                text:     "pub fn args() { todo!() }\n".to_string(),
            },
        ]
    }

    #[test]
    fn a_per_file_lint_sees_a_module_file() {
        // The bug this dispatcher exists for. `ctx.source` is the crate root and
        // nothing else, so every surface lint read `src/lib.rs` and skipped the
        // rest of the crate while the gate reported clean. In one consumer that hid
        // real findings in module files through a full review.
        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;

        let found = check_every_file(&no_todo::NoTodo, &base, &parse_sources(&files), None);
        assert_eq!(
            found.len(),
            1,
            "expected the module file's todo macro, got {found:?}"
        );
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
        let found = check_every_file(&no_todo::NoTodo, &base, &root_only, None);
        assert!(
            found.is_empty(),
            "the crate root is clean, so this should find nothing"
        );
    }

    #[test]
    fn an_excluded_file_is_never_shown_to_a_per_file_lint() {
        // the same dirty fixture as above, which without a filter yields exactly one
        // finding. excluding the file that carries it is the whole mechanism, and the test
        // above is the control that says the finding is there to be suppressed.
        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;
        let filter = PathFilter {
            include: vec![],
            exclude: vec!["src/env.rs".into()],
        };

        let found = check_every_file(
            &no_todo::NoTodo,
            &base,
            &parse_sources(&files),
            Some(&filter),
        );
        assert!(
            found.is_empty(),
            "the only dirty file was excluded, got {found:?}"
        );
    }

    #[test]
    fn a_crate_scoped_lint_cannot_walk_past_its_filter() {
        // the hole worth pinning: a crate-scoped lint ignores the per-file dispatch and
        // reads `all_sources` itself, so unless the runner narrows that too, declaring
        // `per_file = false` is a way around the config.
        struct CountsSources(std::sync::atomic::AtomicUsize);
        impl Lint for CountsSources {
            fn name(&self) -> &'static str {
                "counts-sources"
            }

            fn per_file(&self) -> bool {
                false
            }
        }
        impl CrateLint for CountsSources {
            fn check(&self, ctx: &LintContext) -> Vec<LintError> {
                self.0
                    .store(ctx.all_sources.len(), std::sync::atomic::Ordering::SeqCst);
                Vec::new()
            }
        }

        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;
        let lint = CountsSources(std::sync::atomic::AtomicUsize::new(0));
        let parsed = parse_sources(&files);

        check_every_file(&lint, &base, &parsed, None);
        assert_eq!(
            lint.0.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "unfiltered control"
        );

        let filter = PathFilter {
            include: vec!["src/lib.rs".into()],
            exclude: vec![],
        };
        check_every_file(&lint, &base, &parsed, Some(&filter));
        assert_eq!(
            lint.0.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a crate-scoped lint must be handed the narrowed all_sources, not the full one",
        );
    }

    #[test]
    fn a_filter_that_admits_nothing_skips_the_lint_rather_than_running_it_empty() {
        // "nothing to look at" and "looked and found nothing" are different answers, and a
        // lint like no-empty-crate reports on the second. a filter has to produce the first.
        struct AlwaysFires;
        impl Lint for AlwaysFires {
            fn name(&self) -> &'static str {
                "always-fires"
            }

            fn per_file(&self) -> bool {
                false
            }
        }
        impl CrateLint for AlwaysFires {
            fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
                vec![LintError::error("c".into(), 1, "always-fires", "fired".into())]
            }
        }

        let files = crate_with_a_dirty_module();
        let mut base = ctx();
        base.all_sources = &files;
        let parsed = parse_sources(&files);

        assert_eq!(
            check_every_file(&AlwaysFires, &base, &parsed, None).len(),
            1,
            "control"
        );

        let filter = PathFilter {
            include: vec!["nothing/matches/this".into()],
            exclude: vec![],
        };
        assert!(check_every_file(&AlwaysFires, &base, &parsed, Some(&filter)).is_empty());
    }

    #[test]
    fn the_builtin_set_alone_has_no_duplicate_names() {
        // The gate the host runs on every pack also pins the builtin set
        // itself: two builtins sharing a name would be the same double-report
        // bug with nobody external to blame.
        assert_eq!(
            duplicate_lint_names(&LintPack::default()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_pack_reusing_a_builtin_name_is_reported() {
        // The shipped incident: the stack pack and the builtin set both
        // registered a lint under one name, every finding doubled, and the
        // config could not address either copy.
        struct Shadow;
        impl Lint for Shadow {
            fn name(&self) -> &'static str {
                "no-todo"
            }
        }
        impl CrateLint for Shadow {
            fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
                Vec::new()
            }
        }
        let mut pack = LintPack::default();
        pack.crate_lints.push(Box::new(Shadow));
        assert_eq!(duplicate_lint_names(&pack), vec!["no-todo".to_string()]);
    }

    #[test]
    fn a_duplicate_inside_one_pack_is_reported_too() {
        struct Twin;
        impl Lint for Twin {
            fn name(&self) -> &'static str {
                "twin-lint"
            }
        }
        impl CrateLint for Twin {
            fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
                Vec::new()
            }
        }
        let mut pack = LintPack::default();
        pack.crate_lints.push(Box::new(Twin));
        pack.crate_lints.push(Box::new(Twin));
        assert_eq!(duplicate_lint_names(&pack), vec!["twin-lint".to_string()]);
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
        assert_eq!(
            check_every_file(&CrateScoped, &base, &parse_sources(&files), None).len(),
            1
        );
    }

    fn config_of(name: &str, severity: Severity) -> LintConfig {
        let mut base = HashMap::new();
        base.insert(name.to_string(), severity);
        LintConfig {
            base,
            findings: HashMap::new(),
            params: HashMap::new(),
            paths: PathFilters::new(),
        }
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
        assert_eq!(
            fired(AlwaysFires("declares-error", Severity::HARD_ERROR), None),
            1
        );
    }

    #[test]
    fn a_config_can_turn_on_a_lint_that_declares_off() {
        // Opting in is the whole point of declaring OFF rather than deleting
        // the lint, so the default must not be a floor.
        let cfg = config_of("declares-off", Severity::HARD_ERROR);
        assert_eq!(
            fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)),
            1
        );
    }

    #[test]
    fn a_config_can_turn_off_a_lint_that_declares_a_real_severity() {
        // Pre-existing behaviour, kept.
        let cfg = config_of("declares-error", Severity::OFF);
        assert_eq!(
            fired(
                AlwaysFires("declares-error", Severity::HARD_ERROR),
                Some(&cfg)
            ),
            0
        );
    }

    #[test]
    fn a_config_for_another_lint_does_not_reach_this_one() {
        let cfg = config_of("some-other-lint", Severity::HARD_ERROR);
        assert_eq!(
            fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)),
            0
        );
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
        let cfg = LintConfig {
            base: HashMap::new(),
            findings,
            params: HashMap::new(),
            paths: PathFilters::new(),
        };
        assert_eq!(
            fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)),
            1
        );
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
        let cfg = LintConfig {
            base: HashMap::new(),
            findings: HashMap::new(),
            params,
            paths: PathFilters::new(),
        };
        assert_eq!(
            fired(AlwaysFires("declares-off", Severity::OFF), Some(&cfg)),
            1
        );
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
        assert_eq!(
            fired_across(AlwaysFiresAcross("declares-off", Severity::OFF), None),
            0
        );
        assert_eq!(
            fired_across(
                AlwaysFiresAcross("declares-error", Severity::HARD_ERROR),
                None
            ),
            1
        );
    }

    #[test]
    fn cross_crate_config_can_turn_on_a_lint_that_declares_off() {
        let cfg = config_of("declares-off", Severity::HARD_ERROR);
        assert_eq!(
            fired_across(AlwaysFiresAcross("declares-off", Severity::OFF), Some(&cfg)),
            1
        );
    }
}

#[cfg(test)]
mod no_em_dashes_in_our_own_text {
    /// The workspace forbids em-dashes in every authored surface, and lint
    /// messages are the most user-facing text this crate has: they print into
    /// a consumer's terminal every time a gate fires. Enforcement existed only
    /// for generated agent rules, so this crate accumulated 106 of them while
    /// being the tool that carries the ban.
    ///
    /// Walks the sources rather than checking a rendered artifact, because the
    /// message strings never become one.
    #[test]
    fn this_crate_prints_no_em_dashes_at_a_consumer() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p
                    .extension()
                    .is_some_and(|x| x == "rs" || x == "sh" || x == "md")
                {
                    let Ok(text) = std::fs::read_to_string(&p) else {
                        continue;
                    };
                    for (i, line) in text.lines().enumerate() {
                        if line.contains('\u{2014}') {
                            out.push(format!("{}:{}: {}", p.display(), i + 1, line.trim()));
                        }
                    }
                }
            }
        }

        // Widened past this crate's own `src/`, because the class grep found
        // the same defect in a shell script and a TODO under the repository
        // root. What this still cannot reach is `mock/crates/**/*.md.tmpl`,
        // which is the parked v2 tree and is gated behind a design round; two
        // files there carry em-dashes and are recorded rather than edited.
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("lint-rules sits under the repository root")
            .to_path_buf();
        let mut hits = Vec::new();
        for rel in [
            "lint-rules/src",
            "src",
            "bench-core/src",
            "bench-harness/src",
            "bench-macro/src",
            "bench-matrix/src",
            "cargo-mock/src",
            "scripts",
        ] {
            let dir = repo.join(rel);
            if dir.is_dir() {
                walk(&dir, &mut hits);
            }
        }
        assert!(
            hits.is_empty(),
            "em-dashes are forbidden in authored text, and these reach a \
             consumer's terminal. Use a period, comma, colon, or parentheses:\n{}",
            hits.join("\n")
        );
    }
}
