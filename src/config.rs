//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Configuration for a mockspace workspace.
//!
//! Reads `mockspace.toml` from the mockspace root directory.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use mockspace_lint_rules::{
    Level, LintConfig, PathFilter, PathFilters, Severity, parse_severity,
};
use serde::Deserialize;
use serde::de::IntoDeserializer;

/// How mockspace-managed content is installed into existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallMode {
    Replace,
    MergeAppend,
    MergePrepend,
    Skip,
}

impl InstallMode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "replace" => Some(Self::Replace),
            "merge-append" | "merge_append" | "append" => Some(Self::MergeAppend),
            "merge-prepend" | "merge_prepend" | "prepend" => Some(Self::MergePrepend),
            "skip" | "skip-if-exists" | "skip_if_exists" => Some(Self::Skip),
            _ => None,
        }
    }
}

/// Mockspace workspace configuration.
#[derive(Clone, Debug)]
pub struct Config {
    // --- Core fields ---
    pub mock_dir:               PathBuf,
    /// The `mockspace.toml` this config was read from (in the mock dir or at
    /// the repo root). Needed by callers that re-read the raw config, such as
    /// the runtime custom-lint loader reading `[lint-crates]`.
    pub config_path:            PathBuf,
    /// The first entry of `src_dirs`.
    ///
    /// Never a config key: it was always `mock_dir/crates`, computed rather
    /// than read, so no `mockspace.toml` can name it and there is no second
    /// spelling to conflict with. What it is instead is exported Rust API, via
    /// `pub mod config`, and that is the contract this deprecation is for.
    ///
    /// **On a grouped project this reports only the first group**, which looks
    /// exactly like a smaller project. That is the silent-empty class already
    /// recorded at `parse.rs`, and it is why the replacement is a list rather
    /// than a second path.
    #[deprecated(
        since = "0.0.0",
        note = "use `src_dirs`. This is `src_dirs[0]`, so on a grouped project it \
                sees only the first group and reports a subset as if it were the \
                whole. If one directory really is meant, take it as an argument \
                rather than reading the first entry of a list."
    )]
    pub crates_dir:             PathBuf,
    /// Every directory holding source packages, absolute, in config order.
    ///
    /// One entry for an ordinary project. Several for a grouped one, where the
    /// groups are independent and a package belongs to exactly one.
    pub src_dirs:               Vec<PathBuf>,
    pub repo_root:              PathBuf,
    pub docs_dir:               PathBuf,
    pub project_name:           String,
    pub crate_prefix:           String,
    /// Prepended to every document link a reference renders.
    ///
    /// Empty for a document written into the docs directory, where a bare
    /// filename is the correct relative target. Set for output written
    /// elsewhere, such as the agent rules under `.claude/`, whose links would
    /// otherwise point at siblings that are not there.
    pub doc_link_prefix:        String,
    /// What every generated document will be called, keyed by what a
    /// reference calls it.
    ///
    /// Built from the planned set before anything renders, so a reference from
    /// a document written early to one written later is ordinary rather than a
    /// special case, and so nothing computes a filename a second time.
    pub doc_index:              crate::document::DocIndex,
    pub proc_macro_crates:      Vec<String>,
    /// Whether source-scanning lints should run against proc-macro crate source.
    /// Default false: proc-macro crates run in the compiler host context and
    /// their heap-using parsers do not ship with consumer binaries. Projects
    /// that want to lint proc-macro source anyway (for example to enforce a
    /// consistent style across the workspace) set this to true.
    ///
    /// Independent of expansion-based linting (tracked as a future feature);
    /// the macro's emitted output is always subject to consumer-crate rules
    /// because it compiles into consumer binaries.
    pub lint_proc_macro_source: bool,
    pub module_crates:          Vec<String>,
    pub unprefixed_crates:      Vec<String>,
    pub abi_version:            u32,
    pub nuke_marker:            String,
    /// Declared registry namespaces. Empty when the project declares none, and
    /// every registry code path is a no-op in that case.
    pub registry_namespaces:    Vec<crate::registry::RegistryNamespace>,
    /// Whether to also emit a single document containing every deep dive.
    ///
    /// Off by default. Each deep dive already renders as a sibling of its
    /// crate's overview, which is where a reader looks for it, and the
    /// combined document repeats all of that content in one file that grows
    /// without bound as crates gain deep dives. A project that genuinely wants
    /// one long read can turn it back on.
    pub deep_dive_index:        bool,
    /// Whether generated documents carry a sort prefix.
    ///
    /// A docs directory of thirty crates plus project documents has no
    /// inherent order, so a reader meets it alphabetically, which puts a
    /// leaf crate's deep dive above the document explaining what any of it
    /// is. Prefixing by dependency depth makes the listing read in the order
    /// the architecture is built.
    pub ordered_docs:           bool,
    /// Documents a reader should start with, by output name without extension
    /// (`DESIGN`, `IDENTITY`). These sort first. Everything else that is not
    /// generated from a crate sorts last, as supplementary material.
    pub primary_docs:           Vec<String>,
    /// Named roots for registry provenance references. See `RawRegistry::roots`.
    pub registry_roots:         BTreeMap<String, String>,
    /// Roots declared frozen. Line citations into any other root are reported
    /// as fragile.
    pub frozen_roots:           std::collections::BTreeSet<String>,

    /// Roots whose citations render as prose rather than links, with the name
    /// each goes by.
    pub prose_roots:          BTreeMap<String, String>,
    /// Roots whose citations are dropped from generated documents entirely.
    pub internal_roots:       std::collections::BTreeSet<String>,
    pub install_git_hooks:    InstallMode,
    pub install_cargo_config: InstallMode,
    pub install_agent_files:  InstallMode,

    /// Run `cargo fmt` on the staged workspace roots before a commit is linted,
    /// re-staging the result. Uses the repo's own `rustfmt.toml`. Default true.
    pub auto_fmt:        bool,
    /// Run `cargo clippy --fix` on the staged workspace roots before a commit is
    /// linted, re-staging the result. Uses the repo's own clippy config (the
    /// entrypoint `#![warn(clippy::…)]` attributes). Best-effort. Default true.
    pub auto_clippy_fix: bool,
    /// Run `cargo deny check` on push against the repo's `deny.toml` (advisories,
    /// license compatibility, bans, sources). Blocks on a violation; skipped when
    /// cargo-deny or the config is absent. Default true.
    pub deny_check:      bool,

    /// Everything read from `mock/agent/config.toml`. Grouped rather than
    /// spread across this struct so a reader can tell at a glance which file
    /// a setting comes from; the rest of `Config` is `mockspace.toml`.
    pub agent: AgentConfig,

    // --- Lint overrides ---
    /// Per-lint severity overrides from `[lints]` section.
    /// Key: lint name (e.g. "no-float"), Value: configured severity.
    /// Empty if no `[lints]` section is present (all lints use defaults).
    pub lint_overrides: LintConfig,

    // --- Domain-specific config ---
    /// Macro icon+label for STRUCTURE.md domain items.
    /// e.g. "define_signal" -> "signal"
    pub domain_kinds: BTreeMap<String, String>,

    /// Known macros for DESIGN.md table. (name, description, usage)
    pub known_macros: Vec<(String, String, String)>,

    /// Known macros for agent instructions table. (name, purpose, usage)
    /// If empty, falls back to known_macros.
    pub agent_macros: Vec<(String, String, String)>,

    /// Macro graph styling: name -> (label, icon, bg_color, fg_color)
    pub macro_styles: BTreeMap<String, MacroStyle>,

    /// Crate header colors for graph: short_name -> (bg, fg)
    pub crate_colors: BTreeMap<String, (String, String)>,

    /// Layer labels by depth index.
    pub layer_labels: Vec<String>,

    /// Which macro type to track per-crate for {{signals_per_crate}}.
    pub primary_domain_macro: Option<String>,

    /// Label for the primary domain macro column (e.g. "Signals").
    pub primary_domain_label: String,

    /// Crate companion grouping for graph rank: source -> target.
    pub crate_grouping: BTreeMap<String, String>,

    /// Per-crate primitive-introductions map. Key: crate directory
    /// name (e.g. "arvo", "arvo-bits"). Value: the list of primitive
    /// token names that crate legitimately introduces in its own
    /// source because it is the producer of the wrapped equivalent.
    ///
    /// Bare-primitive lints (`no-bare-numeric`, `arvo-types-only`,
    /// `no-bare-option`, etc.) skip these tokens on these specific
    /// crates. Everything not listed remains subject to the lint.
    ///
    /// Example: `arvo = ["u8", "u16", "u32", "u64", "u128", "i8", ...,
    /// "f32", "f64", "usize", "isize", "bool"]`: arvo defines the
    /// numeric foundations, so it legitimately wraps every std numeric
    /// primitive; meanwhile `Option` / `Result` / `String` still fire
    /// on arvo because arvo does not introduce them.
    ///
    /// # Future direction
    ///
    /// This manual mapping is the belt-and-suspenders path. The
    /// default long-term mechanism should be *detection*, not
    /// *declaration*: mockspace can derive the introductions set for
    /// a crate by either (a) processing the crate's DESIGN.md.tmpl /
    /// README.md.tmpl to extract the documented primitive
    /// definitions, or (b) pre-parsing every `src/**/*.rs` to find
    /// `pub struct USize(pub usize)` / `pub type Byte = ...` and
    /// inferring "this crate introduces `usize` via `USize`". Both
    /// paths stay consistent with the doc = design principle: the
    /// configuration derives from the crate's own contract rather
    /// than living in a parallel TOML table that can drift.
    ///
    /// Until that detection lands, the explicit map wins. When it
    /// lands, the map becomes additive: anything declared here
    /// supplements the detected set rather than replacing it,
    /// letting both paths coexist during the migration.
    pub primitive_introductions: BTreeMap<String, Vec<String>>,

    /// Glob patterns (the same syntax [`mockspace_lint_rules::glob_match`]
    /// already implements for lint path filters) naming this project's own
    /// canon: whatever it considers the settled, ratified design surface
    /// that a panel converges toward but never edits directly.
    ///
    /// Empty by default, which makes the panel-discipline check in
    /// `crate::entry::check` a no-op: a project that has not said what its
    /// canon is has nothing this can protect, and inventing a default (the
    /// most common candidate is a repo-specific convention, not a mockspace
    /// one) would be a guess this crate has no standing to make.
    pub canon_paths: Vec<String>,

    /// How many seats a panel may mint before `mock panel seat` refuses
    /// until a consolidation is recorded. See `crate::panel` for the whole
    /// mechanism this gates.
    pub panel_consolidate_every: u32,
}

/// Everything `mock/agent/config.toml` declares.
///
/// Its own type so the provenance is in the shape rather than in a comment on
/// each field: anything here came from the agent file, and anything on
/// `Config` outside this came from `mockspace.toml`. An absent file yields
/// defaults throughout, so adopting the agent surface is opt-in per setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// Commit byline policy per agent mode.
    pub attribution: AttributionConfig,

    /// Generate the interactive design-talk skill and the script it ships.
    ///
    /// Off by default. The flow presumes a human answering design questions in
    /// the loop, which is not every project's shape, and a skill that leads a
    /// conversation nobody is having is noise in the agent's context.
    pub accelerated_interactive_design_talks: bool,

    /// On by default: the mistake the sketching skill prevents is made in the
    /// first thirty seconds of a task, by every consumer, so it earns a
    /// default. Still a knob, because no builtin prose lands in a consumer's
    /// agent context without one.
    pub sketching_skill: bool,

    /// On by default, same reasoning as `sketching_skill`.
    pub benchmarking_skill: bool,

    /// On by default: a generated rule pointing at `mock tools` rather than
    /// a hand-maintained list. The knob exists for the same reason the other
    /// two do, no builtin prose lands in a consumer's context without one,
    /// not because there is a real cost to leaving it on.
    pub tool_catalogue: bool,

    /// Off by default. Ships the panel-discipline rule: how a panel mints
    /// and consolidates seats, the seat cap, and that a panel converges and
    /// proposes but never writes canon itself. Off by default because the
    /// whole mechanism (`crate::panel`, `mock panel`, `Config::canon_paths`)
    /// presumes a project that runs multi-persona panels at all, which is
    /// not every consumer's shape; a rule describing a workflow nobody is
    /// using is noise in the agent's context, the same reasoning
    /// `accelerated_interactive_design_talks` is off by default for.
    pub panel_discipline: bool,
}

// Not derived: the derive would default every bool to false, and the two
// skill knobs default to on. One impl keeps "absent file" and "absent key"
// meaning the same thing everywhere.
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            attribution: AttributionConfig::default(),
            accelerated_interactive_design_talks: false,
            sketching_skill: true,
            benchmarking_skill: true,
            tool_catalogue: true,
            panel_discipline: false,
        }
    }
}

/// Commit byline policy per agent mode, from `mock/agent/config.toml` `[attribution]`.
///
/// Each field is either an empty string (no byline permitted) or a glob pattern
/// (matching bylines are accepted). Glob patterns support `*`, `?`, `[...]` as
/// in bash `[[ == ]]` pattern matching. Patterns without glob metacharacters
/// degenerate to literal equality.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttributionConfig {
    /// Byline policy when the agent mode env var is unset or "assistant".
    /// Empty string (default): no Co-Authored-By line permitted.
    /// Non-empty pattern: bylines matching this pattern are accepted.
    pub non_autonomous: String,

    /// Byline policy when the agent mode env var is "autonomous".
    /// Must be non-empty at commit time; otherwise the hook fails with a config error.
    /// Commits must carry at least one byline matching this pattern.
    pub autonomous: String,

    /// Tool-advertising-trailer policy, in every mode and on every surface.
    ///
    /// Its own axis, because an advert is not provenance: it is injected by a
    /// tool's defaults rather than chosen, and says nothing about who did the
    /// work. Empty (the default) permits none.
    pub adverts: String,

    /// Ordered signals deciding which ruleset a run falls under. Empty means the
    /// historical behaviour: read `<PROJECT>_AGENT_MODE` and nothing else.
    ///
    /// See [`crate::agent_mode`] for resolution and the available presets.
    pub mode_signals: Vec<crate::agent_mode::RawModeSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroStyle {
    pub label: String,
    pub icon:  String,
    pub bg:    String,
    pub fg:    String,
}

impl MacroStyle {
    #[must_use]
    pub fn default_for(macro_name: &str) -> Self {
        Self {
            label: macro_name
                .strip_prefix("define_")
                .unwrap_or("generated")
                .to_string(),
            icon:  "\u{2699}".to_string(),
            bg:    "#F5F5F5".to_string(),
            fg:    "#616161".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Serde deserialization structs
// ---------------------------------------------------------------------------

/// Raw deserialization struct for `mock/agent/config.toml`.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAgentConfig {
    attribution: Option<RawAttribution>,
    /// Opt in to the interactive design-talk skill, which leads a decision
    /// conversation and ships a script that pulls stranded topics and archived
    /// rounds into the open round.
    agent_accelerated_interactive_design_talks: Option<bool>,

    /// Opt out of the sketching skill builtin; absent means on.
    agent_sketching_skill: Option<bool>,

    /// Opt out of the benchmarking skill builtin; absent means on.
    agent_benchmarking_skill: Option<bool>,

    /// Opt out of the tool-catalogue rule builtin; absent means on.
    agent_tool_catalogue: Option<bool>,

    /// Opt in to the panel-discipline rule builtin; absent means off.
    agent_panel_discipline: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAttribution {
    non_autonomous: Option<String>,
    autonomous:     Option<String>,
    adverts:        Option<String>,
    #[serde(default)]
    mode_signal:    Vec<RawModeSignalToml>,
}

/// One `[[attribution.mode_signal]]` entry, as written.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawModeSignalToml {
    preset: Option<String>,
    env:    Option<String>,
    equals: Option<String>,
    exists: Option<bool>,
    mode:   Option<String>,
}

/// One declared reference root. A table rather than a bare string so a root
/// can gain options without breaking every project that declared one.
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
struct RawRefRoot {
    path:   String,
    /// Whether this root's contents are settled and will not shift.
    ///
    /// Line citations are only honest into a frozen root. Everywhere else an
    /// edit above a cited line silently repoints it, which is the failure this
    /// project treats as worst: the check passes and the answer is wrong.
    /// Declaring a root frozen is a claim that its files do not move, and it
    /// is what turns a line citation from a hazard into a fact.
    frozen: bool,

    /// Whether a citation into this root renders as a link.
    ///
    /// Defaults to true. The question is never whether a root lives under the
    /// design workspace, since a crate and a bibliography entry both do and
    /// both render perfectly well for a reader who has only the published
    /// tree. The question is whether the rendered form leaks an internal path.
    /// A research corpus does; setting this false renders the citation as
    /// prose under `label`, so the provenance survives in the document without
    /// the path doing so.
    #[serde(default = "default_root_links")]
    links: bool,

    /// What this root is called in prose when it does not render as a link.
    /// `seed` is an internal handle that means nothing to a reader; "Prior
    /// research" is what it actually is.
    label: Option<String>,

    /// Whether citations into this root are dropped from generated documents.
    ///
    /// Stronger than `links = false`, which keeps the citation and renders it
    /// as prose. Some roots are worth citing in the source and worth nothing
    /// in the result: a reader who cannot open the corpus gains nothing from
    /// being told a row came from it, and a provenance column that says
    /// "Prior research" on every row says nothing on any of them.
    ///
    /// Filtering is per item rather than per cell, so a row citing both this
    /// root and a public one keeps the public citation. A cell left empty by
    /// filtering renders empty, and a column left empty across every row drops
    /// entirely, which is the existing rule for a column no row carries.
    #[serde(default)]
    internal: bool,
}

fn default_root_links() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
struct RawRef {
    /// Named roots a citation may use, relative to the repository root. The
    /// root name is what disambiguates a project's own document from a corpus
    /// it indexes when both are called the same thing.
    roots: BTreeMap<String, RawRefRoot>,
}

#[derive(Debug, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
struct RawRegistry {
    #[serde(rename = "namespace")]
    namespace: Vec<crate::registry::RegistryNamespace>,
}

#[derive(Deserialize, Default)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
struct RawConfig {
    project_name:           Option<String>,
    crate_prefix:           Option<String>,
    abi_version:            Option<u32>,
    /// Directories under the mock dir that hold source packages, relative to
    /// it. Empty means the default, a single `crates`.
    ///
    /// Several entries is a project whose packages are grouped: one entry per
    /// group, and the groups are independent. Entries are paths rather than
    /// names, so a project keeps whatever nesting it already had.
    #[serde(default)]
    src_dirs:               Vec<String>,
    proc_macro_crates:      Vec<String>,
    #[serde(default)]
    lint_proc_macro_source: Option<bool>,
    module_crates:          Vec<String>,
    #[serde(default)]
    unprefixed_crates:      Vec<String>,
    layers:                 Vec<String>,
    primary_domain_macro:   Option<String>,
    primary_domain_label:   Option<String>,
    install_git_hooks:      Option<String>,
    install_cargo_config:   Option<String>,
    install_agent_files:    Option<String>,

    #[serde(default)]
    auto_fmt:        Option<bool>,
    #[serde(default)]
    auto_clippy_fix: Option<bool>,
    #[serde(default)]
    deny_check:      Option<bool>,

    // Sections (simple key=value maps)
    domain_kinds:   Option<BTreeMap<String, String>>,
    known_macros:   Option<BTreeMap<String, String>>,
    agent_macros:   Option<BTreeMap<String, String>>,
    macro_styles:   Option<BTreeMap<String, String>>,
    crate_colors:   Option<BTreeMap<String, String>>,
    crate_grouping: Option<BTreeMap<String, String>>,

    // Primitive-introductions map: crate_name -> list of primitive
    // tokens that the named crate legitimately introduces. Consumed
    // by bare-primitive lints to skip per-primitive per-crate.
    #[serde(rename = "primitive-introductions", alias = "primitive_introductions")]
    primitive_introductions: Option<BTreeMap<String, Vec<String>>>,

    /// Glob patterns naming this project's own canon. See
    /// `Config::canon_paths`.
    #[serde(default)]
    canon_paths: Vec<String>,

    /// How many seats a panel mints before a consolidation is required.
    /// Absent means the built-in default; see `crate::panel::DEFAULT_CONSOLIDATE_EVERY`.
    #[serde(default)]
    panel_consolidate_every: Option<u32>,

    /// Registry namespaces: the kinds of thing this project's documents refer
    /// to by identifier. Absent means the project does not use the registry,
    /// which must stay a no-cost default.
    #[serde(default)]
    registry: Option<RawRegistry>,

    /// Opt in to the combined deep-dive document. See `Config::deep_dive_index`.
    #[serde(default)]
    deep_dive_index: Option<bool>,

    /// Reference roots. See `RawRef`.
    #[serde(default, rename = "ref")]
    ref_cfg: Option<RawRef>,

    /// Opt in to sort prefixes on generated documents.
    #[serde(default)]
    ordered_docs: Option<bool>,

    /// Documents a reader should start with. See `Config::primary_docs`.
    #[serde(default)]
    primary_docs: Vec<String>,
    // Lints section is handled separately via toml_edit document API
    // because it contains heterogeneous values (strings and tables).
}

/// A lint entry can be either a preset string or a table with config.
#[derive(Deserialize)]
#[serde(untagged)]
enum LintEntry {
    Preset(String),
    Config(LintTableConfig),
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LintTableConfig {
    commit:   Option<String>,
    build:    Option<String>,
    push:     Option<String>,
    severity: Option<String>,
    findings: Option<BTreeMap<String, String>>,
    /// Path globs this lint may be shown, and ones it may not. Named explicitly rather than
    /// left to the flattened `params` map, because both are arrays and everything that map
    /// holds is a string.
    include:  Option<Vec<String>>,
    exclude:  Option<Vec<String>>,
    rule:     Option<BTreeMap<String, BTreeMap<String, StringOrOther>>>,
    /// Inline array of tables format:
    /// `rules = [{ scope = "...", forbidden = "...", reason = "..." }]`
    rules:    Option<Vec<InlineRule>>,
    #[serde(flatten)]
    params:   BTreeMap<String, StringOrOther>,
}

/// A single forbidden-imports rule in inline array format.
#[derive(Deserialize)]
struct InlineRule {
    scope:     String,
    forbidden: String,
    reason:    String,
    #[serde(default = "default_true")]
    enabled:   bool,
}

fn default_true() -> bool {
    true
}

/// Helper to deserialize heterogeneous TOML values as strings.
/// Handles string, integer, float, and boolean values.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum StringOrOther {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
}

impl StringOrOther {
    fn into_string(self) -> String {
        match self {
            StringOrOther::String(s) => s,
            StringOrOther::Integer(i) => i.to_string(),
            StringOrOther::Float(f) => f.to_string(),
            StringOrOther::Bool(b) => b.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Config construction
// ---------------------------------------------------------------------------

impl Config {
    #[must_use]
    pub fn from_dir(mock_dir: &Path) -> Self {
        let mock_dir = mock_dir.to_path_buf();

        let repo_root = find_repo_root(&mock_dir).unwrap_or_else(|| mock_dir.clone());
        let docs_dir = repo_root.join("docs");

        // The config sits either in the mock dir (in place, the historical
        // location) or at the repo root (its home once relocated). Prefer the
        // in-place one so an un-relocated repo is unchanged; else read the
        // root one. The mock dir itself always comes from the caller (the
        // launcher resolves it), so only the config's read location varies.
        let in_mock = mock_dir.join("mockspace.toml");
        let at_root = repo_root.join("mockspace.toml");
        let toml_path = if in_mock.is_file() {
            in_mock
        } else if at_root.is_file() {
            at_root
        } else {
            in_mock
        };
        let toml_content = fs::read_to_string(&toml_path).unwrap_or_default();

        let raw: RawConfig = toml_edit::de::from_str(&toml_content).unwrap_or_default();

        let project_name = raw.project_name.unwrap_or_else(|| "project".into());
        let crate_prefix = raw.crate_prefix.unwrap_or_else(|| project_name.clone());

        // A declared list rather than a walked tree, because a walk cannot tell
        // an empty group from a directory that is not a group, and this project
        // has already shipped one silent-empty discovery. A named directory that
        // is missing is a configuration error and says so; an absent
        // subdirectory in a walk says nothing at all.
        let src_dirs: Vec<PathBuf> = if raw.src_dirs.is_empty() {
            vec![mock_dir.join("crates")]
        } else {
            raw.src_dirs.iter().map(|d| mock_dir.join(d)).collect()
        };
        if raw.src_dirs.is_empty() {
            // The default is allowed not to exist: a project with no packages
            // yet is ordinary, and discovery yields nothing without complaint.
        } else {
            let missing: Vec<_> = src_dirs.iter().filter(|d| !d.is_dir()).collect();
            assert!(
                missing.is_empty(),
                "mockspace.toml names src_dirs that are not directories: {}\n  \
                 Every entry is relative to the mock dir ({}). An entry naming \
                 nothing would contribute no packages silently, so it is refused \
                 here instead.",
                missing
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                mock_dir.display(),
            );
        }
        let crates_dir = src_dirs[0].clone();

        let install_git_hooks = raw
            .install_git_hooks
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);
        let install_cargo_config = raw
            .install_cargo_config
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::MergeAppend);
        let install_agent_files = raw
            .install_agent_files
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);

        let domain_kinds = raw.domain_kinds.unwrap_or_default();

        let known_macros = pipe2_section(raw.known_macros);
        let agent_macros = pipe2_section(raw.agent_macros);

        let macro_styles = convert_macro_styles(raw.macro_styles, &domain_kinds);
        let crate_colors = convert_color_section(raw.crate_colors);

        let lint_overrides = parse_lints_from_document(&toml_content, &crate_prefix);

        let primary_domain_label = raw
            .primary_domain_label
            .unwrap_or_else(|| "Items".to_string());

        // Load mock/agent/config.toml if present. Empty defaults when absent.
        let agent_config_path = mock_dir.join("agent").join("config.toml");
        let agent_config_content = fs::read_to_string(&agent_config_path).unwrap_or_default();
        let raw_agent: RawAgentConfig =
            toml_edit::de::from_str(&agent_config_content).unwrap_or_default();
        let attribution = raw_agent
            .attribution
            .map(|a| {
                AttributionConfig {
                    non_autonomous: a.non_autonomous.unwrap_or_default(),
                    autonomous:     a.autonomous.unwrap_or_default(),
                    adverts:        a.adverts.unwrap_or_default(),
                    mode_signals:   a
                        .mode_signal
                        .into_iter()
                        .map(|s| {
                            crate::agent_mode::RawModeSignal {
                                preset: s.preset,
                                env:    s.env,
                                equals: s.equals,
                                exists: s.exists,
                                mode:   s.mode,
                            }
                        })
                        .collect(),
                }
            })
            .unwrap_or_default();

        let agent = AgentConfig {
            attribution,
            accelerated_interactive_design_talks: raw_agent
                .agent_accelerated_interactive_design_talks
                .unwrap_or(false),
            sketching_skill: raw_agent.agent_sketching_skill.unwrap_or(true),
            benchmarking_skill: raw_agent.agent_benchmarking_skill.unwrap_or(true),
            tool_catalogue: raw_agent.agent_tool_catalogue.unwrap_or(true),
            panel_discipline: raw_agent.agent_panel_discipline.unwrap_or(false),
        };

        // Builtins first, then the project's own, so a project may repoint a
        // builtin root but never has to declare one to get started.
        let mock_rel = mock_dir
            .strip_prefix(&repo_root)
            .unwrap_or(&mock_dir)
            .to_string_lossy()
            .replace('\\', "/");
        let mut registry_roots_raw = crate::registry::builtin_roots(&mock_rel);
        if let Some(r) = raw.ref_cfg.as_ref() {
            for (name, root) in &r.roots {
                registry_roots_raw.insert(name.clone(), root.path.clone());
            }
        }
        Config {
            mock_dir,
            crates_dir,
            src_dirs,
            repo_root,
            docs_dir,
            config_path: toml_path,
            project_name,
            crate_prefix,
            doc_link_prefix: String::new(),
            doc_index: crate::document::DocIndex::default(),
            proc_macro_crates: raw.proc_macro_crates,
            lint_proc_macro_source: raw.lint_proc_macro_source.unwrap_or(false),
            module_crates: raw.module_crates,
            unprefixed_crates: raw.unprefixed_crates,
            abi_version: raw.abi_version.unwrap_or(1),
            nuke_marker: "Nuked by `cargo mock --nuke`".to_string(),
            registry_namespaces: crate::registry::with_builtins(
                &raw.registry.map(|r| r.namespace).unwrap_or_default(),
            ),
            deep_dive_index: raw.deep_dive_index.unwrap_or(false),
            ordered_docs: raw.ordered_docs.unwrap_or(false),
            primary_docs: raw.primary_docs,
            registry_roots: registry_roots_raw,
            prose_roots: raw
                .ref_cfg
                .as_ref()
                .map(|r| {
                    r.roots
                        .iter()
                        .filter(|(_, v)| !v.links)
                        .map(|(k, v)| (k.clone(), v.label.clone().unwrap_or_else(|| k.clone())))
                        .collect()
                })
                .unwrap_or_default(),
            internal_roots: raw
                .ref_cfg
                .as_ref()
                .map(|r| {
                    r.roots
                        .iter()
                        .filter(|(_, v)| v.internal)
                        .map(|(k, _)| k.clone())
                        .collect()
                })
                .unwrap_or_default(),
            frozen_roots: raw
                .ref_cfg
                .as_ref()
                .map(|r| {
                    r.roots
                        .iter()
                        .filter(|(_, v)| v.frozen)
                        .map(|(k, _)| k.clone())
                        .collect()
                })
                .unwrap_or_default(),
            install_git_hooks,
            install_cargo_config,
            install_agent_files,
            auto_fmt: raw.auto_fmt.unwrap_or(true),
            auto_clippy_fix: raw.auto_clippy_fix.unwrap_or(true),
            deny_check: raw.deny_check.unwrap_or(true),
            agent,
            lint_overrides,
            domain_kinds,
            known_macros,
            agent_macros,
            macro_styles,
            crate_colors,
            layer_labels: raw.layers,
            primary_domain_macro: raw.primary_domain_macro,
            primary_domain_label,
            crate_grouping: raw.crate_grouping.unwrap_or_default(),
            primitive_introductions: raw.primitive_introductions.unwrap_or_default(),
            canon_paths: raw.canon_paths,
            panel_consolidate_every: raw
                .panel_consolidate_every
                .unwrap_or(crate::panel::DEFAULT_CONSOLIDATE_EVERY),
        }
    }

    /// Get domain kind label for a macro name (e.g. "define_signal" -> "signal").
    #[must_use]
    pub fn domain_kind(&self, macro_name: &str) -> String {
        if let Some(label) = self.domain_kinds.get(macro_name) {
            return label.clone();
        }
        match macro_name.strip_prefix("define_") {
            Some(kind) => format!("\u{2699} {kind}"),
            None => "\u{2699} generated".to_string(),
        }
    }

    /// Get graph style for a macro name.
    #[must_use]
    pub fn macro_style(&self, macro_name: &str) -> MacroStyle {
        self.macro_styles
            .get(macro_name)
            .cloned()
            .unwrap_or_else(|| MacroStyle::default_for(macro_name))
    }

    /// Get crate header colors for graph. Returns (bg, fg).
    #[must_use]
    pub fn crate_color(&self, short_name: &str) -> (String, String) {
        self.crate_colors
            .get(short_name)
            .cloned()
            .unwrap_or_else(|| ("#F0F0F0".to_string(), "#666666".to_string()))
    }

    /// Get layer label for a depth index.
    #[must_use]
    pub fn layer_label(&self, depth: usize) -> String {
        self.layer_labels
            .get(depth)
            .cloned()
            .unwrap_or_else(|| "Other".to_string())
    }

    /// Get the effective known_macros for agent templates.
    /// Returns agent_macros if non-empty, otherwise known_macros.
    #[must_use]
    pub fn effective_agent_macros(&self) -> &[(String, String, String)] {
        if self.agent_macros.is_empty() {
            &self.known_macros
        } else {
            &self.agent_macros
        }
    }
}

// ---------------------------------------------------------------------------
// Post-processing helpers
// ---------------------------------------------------------------------------

/// Convert a pipe-separated 2-field section: `key = "field1 | field2"`
/// Returns Vec<(key, field1, field2)> preserving BTreeMap ordering.
fn pipe2_section(raw: Option<BTreeMap<String, String>>) -> Vec<(String, String, String)> {
    let raw = match raw {
        Some(m) => m,
        None => return Vec::new(),
    };
    raw.into_iter()
        .map(|(key, val)| {
            let parts: Vec<&str> = val.splitn(2, '|').map(|s| s.trim()).collect();
            match parts.len() {
                1 => (key, parts[0].to_string(), String::new()),
                _ => (key, parts[0].to_string(), parts[1].to_string()),
            }
        })
        .collect()
}

/// Convert `[macro_styles]` pipe-separated values into `MacroStyle` structs.
/// Format: `define_foo = "label | icon | bg_color | fg_color"`
fn convert_macro_styles(
    raw: Option<BTreeMap<String, String>>,
    domain_kinds: &BTreeMap<String, String>,
) -> BTreeMap<String, MacroStyle> {
    let raw = match raw {
        Some(m) => m,
        None => return BTreeMap::new(),
    };
    let mut result = BTreeMap::new();
    for (name, val) in raw {
        let parts: Vec<&str> = val.splitn(4, '|').map(|s| s.trim()).collect();
        if parts.len() >= 4 {
            result.insert(name, MacroStyle {
                label: parts[0].to_string(),
                icon:  parts[1].to_string(),
                bg:    parts[2].to_string(),
                fg:    parts[3].to_string(),
            });
        } else if parts.len() == 3 {
            // 3-field: icon | bg | fg (label from domain_kinds)
            let label = domain_kinds
                .get(&name)
                .map(|dk| {
                    dk.chars()
                        .skip_while(|c| !c.is_ascii_alphanumeric())
                        .collect::<String>()
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| name.strip_prefix("define_").unwrap_or(&name).to_string());
            result.insert(name, MacroStyle {
                label,
                icon: parts[0].to_string(),
                bg: parts[1].to_string(),
                fg: parts[2].to_string(),
            });
        }
    }
    result
}

/// Convert a pipe-separated color pair section: `key = "bg | fg"`
fn convert_color_section(
    raw: Option<BTreeMap<String, String>>,
) -> BTreeMap<String, (String, String)> {
    let raw = match raw {
        Some(m) => m,
        None => return BTreeMap::new(),
    };
    let mut result = BTreeMap::new();
    for (name, val) in raw {
        let parts: Vec<&str> = val.splitn(2, '|').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            result.insert(name, (parts[0].to_string(), parts[1].to_string()));
        }
    }
    result
}

/// Parse the `[lints]` section from the TOML content using `toml_edit`'s
/// document API to handle heterogeneous values (strings and tables).
///
/// Each lint entry is extracted as a `toml_edit::Value` and then deserialized
/// via `IntoDeserializer` into a `LintEntry` enum.
fn parse_lints_from_document(toml_content: &str, crate_prefix: &str) -> LintConfig {
    let doc = match toml_content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return LintConfig::empty(),
    };

    let lints_item = match doc.get("lints") {
        Some(item) => item,
        None => return LintConfig::empty(),
    };

    let lints_table = match lints_item.as_table() {
        Some(t) => t,
        None => return LintConfig::empty(),
    };

    let mut base = HashMap::new();
    let mut findings: HashMap<String, HashMap<String, Severity>> = HashMap::new();
    let mut params: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut paths = PathFilters::new();

    for (lint_name, item) in lints_table.iter() {
        let lint_name = lint_name.to_string();

        // Deserialize the lint entry.
        // For inline values (strings, inline tables), use Value's IntoDeserializer.
        // For standard tables ([lints.name] sections), serialize to a TOML
        // fragment and re-parse via serde.
        let entry: LintEntry = if let Some(v) = item.as_value() {
            match LintEntry::deserialize(v.clone().into_deserializer()) {
                Ok(e) => e,
                Err(_) => continue,
            }
        } else if let Some(tbl) = item.as_table() {
            // Build a mini TOML document with just this table's contents
            let mut doc = toml_edit::DocumentMut::new();
            for (k, v) in tbl.iter() {
                doc[k] = v.clone();
            }
            match toml_edit::de::from_str::<LintTableConfig>(&doc.to_string()) {
                Ok(cfg) => LintEntry::Config(cfg),
                Err(_) => continue,
            }
        } else {
            continue;
        };

        match entry {
            LintEntry::Preset(s) => {
                if let Some(severity) = parse_severity(&s) {
                    base.insert(lint_name, severity);
                }
            },
            LintEntry::Config(table) => {
                // Determine base severity from "severity" key
                if let Some(ref s) = table.severity {
                    if let Some(severity) = parse_severity(s) {
                        base.insert(lint_name.clone(), severity);
                    }
                }

                // Per-gate severity from commit/build/push keys
                let has_gates =
                    table.commit.is_some() || table.build.is_some() || table.push.is_some();
                if has_gates {
                    let entry = base.entry(lint_name.clone()).or_insert(Severity::OFF);
                    if let Some(ref s) = table.commit {
                        if let Some(level) = Level::from_str_name(s) {
                            entry.on_commit = level;
                        }
                    }
                    if let Some(ref s) = table.build {
                        if let Some(level) = Level::from_str_name(s) {
                            entry.on_build = level;
                        }
                    }
                    if let Some(ref s) = table.push {
                        if let Some(level) = Level::from_str_name(s) {
                            entry.on_push = level;
                        }
                    }
                }

                // Per-finding severity overrides
                if let Some(finding_map) = table.findings {
                    let entry = findings.entry(lint_name.clone()).or_default();
                    for (kind, val) in finding_map {
                        if let Some(severity) = parse_severity(&val) {
                            entry.insert(kind, severity);
                        }
                    }
                }

                // Path scoping. `{prefix}` expands here the same way it does in params, so
                // a shared lint pack can name a project's own crates in a glob.
                if table.include.is_some() || table.exclude.is_some() {
                    let expand = |v: Vec<String>| -> Vec<String> {
                        v.into_iter().map(|s| s.replace("{prefix}", crate_prefix)).collect()
                    };
                    paths.insert(lint_name.clone(), PathFilter {
                        include: expand(table.include.unwrap_or_default()),
                        exclude: expand(table.exclude.unwrap_or_default()),
                    });
                }

                // Named rule sub-tables: [lints.lint-name.rule.rule-name]
                if let Some(rule_map) = table.rule {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (rule_name, rule_fields) in rule_map {
                        for (key, val) in rule_fields {
                            let val_str = val.into_string().replace("{prefix}", crate_prefix);
                            let param_key = format!("rule.{rule_name}.{key}");
                            param_entry.insert(param_key, val_str);
                        }
                    }
                }

                // Inline rules array: rules = [{ scope, forbidden, reason }]
                if let Some(rules_array) = table.rules {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (idx, rule) in rules_array.into_iter().enumerate() {
                        if !rule.enabled {
                            continue;
                        }
                        let scope = rule.scope.replace("{prefix}", crate_prefix);
                        let forbidden = rule.forbidden.replace("{prefix}", crate_prefix);
                        let reason = rule.reason.replace("{prefix}", crate_prefix);
                        let name = format!("rule-{idx}");
                        param_entry.insert(format!("rule.{name}.scope"), scope);
                        param_entry.insert(format!("rule.{name}.forbidden"), forbidden);
                        param_entry.insert(format!("rule.{name}.reason"), reason);
                    }
                }

                // Remaining flattened params
                if !table.params.is_empty() {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (key, val) in table.params {
                        let val_str = val.into_string().replace("{prefix}", crate_prefix);
                        param_entry.insert(key, val_str);
                    }
                }
            },
        }
    }

    LintConfig {
        base,
        findings,
        params,
        paths,
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod key_lists_cannot_rot {
    //! The two hand-kept key lists in `registry::validate` mirror the structs
    //! here. A list nobody checks is a comment with a type, and that file says
    //! so about its neighbour fifteen lines above one of them.
    //!
    //! So neither list is trusted: a real instance is serialised and every key
    //! it emits has to be in the list. A field added to either struct reaches
    //! the emitted TOML on its own, and nothing here has to remember it exists.
    use super::*;

    /// Every key the struct has, including the `Option` ones that are `None`.
    ///
    /// NOTE: json rather than toml, and it matters. `toml_edit::ser` omits a
    /// `None`, so a new `Option` field would emit nothing and this would stay
    /// green while a config declaring that key was reported as unknown, which
    /// is the exact failure these tests exist to catch. `serde_json` emits it
    /// as null, so the key is there whether or not it has a value.
    fn declared_keys<T: serde::Serialize>(v: &T) -> Vec<String> {
        match serde_json::to_value(v).expect("serialises") {
            serde_json::Value::Object(m) => m.keys().cloned().collect(),
            other => panic!("expected an object, got {other}"),
        }
    }

    #[test]
    fn every_key_a_ref_root_carries_is_in_ref_root_keys() {
        let keys = declared_keys(&RawRefRoot::default());
        assert!(!keys.is_empty(), "control: the instance declares something");
        for k in &keys {
            assert!(
                crate::registry::validate::REF_ROOT_KEYS.contains(&k.as_str()),
                "`{k}` is a field of RawRefRoot and is missing from REF_ROOT_KEYS, so a \
                 config declaring it would be reported as unknown and fail the gate"
            );
        }
    }

    #[test]
    fn ref_root_keys_names_nothing_the_struct_dropped() {
        // the other direction. a key left in the list after its field went
        // away is a key the check silently permits forever.
        let keys = declared_keys(&RawRefRoot::default());
        for k in crate::registry::validate::REF_ROOT_KEYS {
            assert!(
                keys.iter().any(|e| e == k),
                "REF_ROOT_KEYS names `{k}`, which RawRefRoot no longer carries"
            );
        }
    }

    #[test]
    fn every_key_the_launcher_reads_is_in_root_keys() {
        // these live in `mockspace_manifest` because the launcher reads them
        // before the engine exists. Omitting them made this check report every
        // project's own engine pin as an unknown key.
        let keys = declared_keys(&mockspace_manifest::ManifestHeader::default());
        assert!(!keys.is_empty(), "control: the header declares something");
        for k in &keys {
            assert!(
                crate::registry::validate::ROOT_KEYS.contains(&k.as_str()),
                "`{k}` is a field of ManifestHeader and is missing from ROOT_KEYS"
            );
        }
    }

    #[test]
    fn every_key_the_root_carries_is_in_root_keys() {
        let keys = declared_keys(&RawConfig::default());
        assert!(!keys.is_empty(), "control: the instance declares something");
        for k in &keys {
            assert!(
                crate::registry::validate::ROOT_KEYS.contains(&k.as_str()),
                "`{k}` is a field of RawConfig and is missing from ROOT_KEYS, so a config \
                 declaring it would be reported as unknown and fail the gate"
            );
        }
    }

    #[test]
    fn root_keys_names_nothing_the_struct_dropped() {
        let keys = declared_keys(&RawConfig::default());
        let launcher = declared_keys(&mockspace_manifest::ManifestHeader::default());
        for k in crate::registry::validate::ROOT_KEYS {
            // `lints` is read through toml_edit rather than through RawConfig,
            // deliberately, its values being heterogeneous. It is the one
            // documented exception and the test knows it by name rather than
            // by a rule that would hide the next one.
            if *k == "lints" || launcher.iter().any(|l| l == k) {
                continue;
            }
            assert!(
                keys.iter().any(|e| e == k),
                "ROOT_KEYS names `{k}`, which RawConfig no longer carries"
            );
        }
    }
}
