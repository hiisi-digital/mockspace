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
    CrateFilters,
    Level,
    LintConfig,
    PathFilter,
    PathFilters,
    SEVERITY_NAMES,
    Severity,
    parse_severity,
};
use serde::Deserialize;

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

/// The text a nuked crate's `lib.rs` carries. The config holds it as `nuke_marker`, and
/// nothing in `mockspace.toml` sets it, so this default is the only value it takes.
///
/// The engine's nuke detection and the generated hooks both read the marker the
/// config resolves to, and this is the one place the default is spelled.
pub const DEFAULT_NUKE_MARKER: &str = "Nuked by `cargo mock --nuke`";

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
    /// The `append_only` patterns of `mockspace.toml`, as written.
    pub append_only:            Vec<String>,
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
    /// The clone's own `git config mockspace.autoFmt` beats the file.
    pub auto_fmt:        bool,
    /// Run `cargo clippy --fix` on the staged workspace roots before a commit is
    /// linted, re-staging the result. Uses the repo's own clippy config (the
    /// entrypoint `#![warn(clippy::…)]` attributes). Best-effort. Default true.
    /// The clone's own `git config mockspace.autoClippyFix` beats the file.
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
    /// name (e.g. "numerics", "numerics-bits"). Value: the list of primitive
    /// token names that crate legitimately introduces in its own
    /// source because it is the producer of the wrapped equivalent.
    ///
    /// A lint that refuses bare primitives skips these tokens on these
    /// specific crates. Everything not listed remains subject to it. No such
    /// lint ships here: they come from a pack a project imports if it wants
    /// them, and this key is the engine's half of the arrangement.
    ///
    /// Example: `numerics = ["u8", "u16", "u32", "u64", "u128", "i8", ...,
    /// "f32", "f64", "usize", "isize", "bool"]`: that crate defines the
    /// numeric foundations, so it legitimately wraps every std numeric
    /// primitive; meanwhile `Option` / `Result` / `String` still fire
    /// on it because it does not introduce them.
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

    /// `[test]`: what `mock test` counts as heavy, how many heavy tests run at
    /// once, when a test is killed, and what a commit runs. See
    /// `crate::suite::settings`.
    pub test: crate::suite::settings::TestSettings,
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

    /// On by default: a generated rule pointing at `mock tools` and
    /// `mock lints` rather than at a hand-maintained list of either. The knob
    /// exists for the same reason the other two do, no builtin prose lands in
    /// a consumer's context without one, not because there is a real cost to
    /// leaving it on.
    ///
    /// **Both catalogues, under one key.** They answer one question in two
    /// halves, what is available here and what is checking you, and a
    /// consumer wanting the rule at all wants both halves of it. Splitting
    /// them would mean a project could ship an agent rule that names the tools
    /// and leaves the agent to grep for the lints, which is the thing the rule
    /// exists to stop.
    pub catalogues: bool,

    /// Off by default. Ships the panel-discipline rule: how a panel mints
    /// and consolidates seats, the seat cap, and that a panel converges and
    /// proposes but never writes canon itself. Off by default because the
    /// whole mechanism (`crate::panel`, `mock panel`, `Config::canon_paths`)
    /// presumes a project that runs multi-persona panels at all, which is
    /// not every consumer's shape; a rule describing a workflow nobody is
    /// using is noise in the agent's context, the same reasoning
    /// `accelerated_interactive_design_talks` is off by default for.
    pub panel_discipline: bool,

    /// What the load had to say about keys this config still spells the old
    /// way, one string per complaint, empty when there is nothing to say.
    ///
    /// The complaint goes to stderr as it is found, and printing was all it
    /// did for a while, which is a thing no test can hold onto: commenting the
    /// call out left the suite green, because every test that knew about the
    /// rename called the helper directly and none of them went through a load.
    /// Recording it here is what lets a test ask a real `from_dir` whether it
    /// noticed.
    pub renamed_keys: Vec<String>,
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
            catalogues: true,
            panel_discipline: false,
            renamed_keys: Vec::new(),
        }
    }
}

/// Commit byline policy per agent mode, from `mock/agent/config.toml` `[attribution]`.
///
/// Each field is either an empty string (no byline permitted) or a glob pattern
/// (matching bylines are accepted). Glob patterns support `*`, `?`, `[...]` as
/// in bash `[[ == ]]` pattern matching. Patterns without glob metacharacters
/// degenerate to literal equality.
///
/// The patterns are matched by a lint pack's attribution lint, and mockspace
/// hands that lint more than the message: the commit's author and committer
/// arrive with it, at the commit gate and at the push gate. A pack that judges an
/// agent identity does so under the pattern its mode permits, the way it judges
/// a `Co-Authored-By` trailer, so a mode permitting no byline permits no agent
/// identity either.
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

/// What to say about a config still setting a key that has been renamed.
///
/// `Some` when the old key is present, and the text names the new one.
///
/// **Split out so it can be tested.** `Config::from_dir` returns `Self` with no
/// error channel and nothing in this crate exits from a config load, so the
/// refusal is a message rather than a stop, and a message nobody asserts is a
/// message that quietly stops being printed. What must not happen is silence:
/// `#[serde(default)]` with no `deny_unknown_fields` drops an unknown key
/// without a word, so a consumer who wrote `agent_tool_catalogue = false` gets
/// the rule shipped anyway and never learns their opt-out went unread.
///
/// **Not an alias.** The value is deliberately not consulted. Reading it would
/// hand a consumer the wider rule they never asked for, and pre-1.0 the old
/// shape is deleted rather than carried.
fn renamed_agent_key(old_key_present: bool) -> Option<String> {
    old_key_present.then(|| {
        "mock: `agent_tool_catalogue` in mock/agent/config.toml has been renamed to \
         `agent_catalogues`.\n  The rule it gates now lists the lints as well as the tools, so \
         the old name no longer describes it.\n  The old key is NOT read: rename it, or the rule \
         ships despite your opt-out."
            .to_string()
    })
}

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

    /// Opt out of the catalogues rule builtin; absent means on.
    ///
    /// The rule covers tools and lints, so the key names neither. It was
    /// `agent_tool_catalogue` while the rule listed only tools, and a consumer
    /// who set that to `false` for the reason the name gave would now be
    /// dropping the lint half without having said so.
    agent_catalogues: Option<bool>,

    /// The name that key had, kept only so a config still using it is refused.
    ///
    /// **Not an alias.** Reading it as one would silently give a consumer the
    /// wider rule they never asked for, and pre-1.0 the old shape is deleted
    /// rather than carried. What is not acceptable is deleting it into silence:
    /// `#[serde(default)]` with no `deny_unknown_fields` drops an unknown key
    /// without a word, so a consumer who wrote `agent_tool_catalogue = false`
    /// gets the rule shipped anyway and nothing tells them their opt-out
    /// stopped being read. The field exists so [`Config::from_dir`] can refuse
    /// and name the rename.
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
    /// Files that only ever grow, as `.gitattributes` patterns relative to the
    /// repository root. Each is sent to the `mockspace-append` merge driver, which
    /// keeps both sides' additions when neither side touched a line already there.
    #[serde(default)]
    append_only:            Vec<String>,
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

    /// `[test]`. See `Config::test`.
    #[serde(default)]
    test: Option<crate::suite::settings::RawTest>,

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

/// A lint entry is either a preset string or a table with config, told apart by
/// the TOML type of the value, so a wrong value is reported against its own field
/// and never as "did not match any variant".
enum LintEntry {
    Preset(String),
    Config(LintTableConfig),
}

/// One `[lints.<name>]` table, read key by key by `read_lint_table`.
///
/// Not a `Deserialize` struct: serde reports a value that fits none of a field's
/// types in words about its own machinery (`untagged enum`, `missing field`) and
/// loses which key held it, so every value here is converted by hand and a
/// refusal names the key and the types it takes.
#[derive(Default)]
struct LintTableConfig {
    commit:        Option<String>,
    build:         Option<String>,
    push:          Option<String>,
    severity:      Option<String>,
    findings:      Option<BTreeMap<String, String>>,
    /// Path globs this lint may be shown, and ones it may not. Named explicitly rather than
    /// left to the `params` map, because both are arrays and everything that map holds is a
    /// scalar.
    include:       Option<Vec<String>>,
    exclude:       Option<Vec<String>>,
    // Which crates the lint binds, as globs over the crate name. `crates`
    // admits, `exempt_crates` removes, and a lint naming neither binds every
    // crate. The names match the scope block the engine rewrite reads, so a
    // configuration written for one is read by the other.
    crates:        Option<Vec<String>>,
    exempt_crates: Option<Vec<String>>,
    /// `[lints.<name>.rule.<rule>]` sub-tables, flattened to `rule -> key -> value`.
    rule:          Option<BTreeMap<String, BTreeMap<String, String>>>,
    /// Inline array of tables format:
    /// `rules = [{ scope = "...", forbidden = "...", reason = "..." }]`
    rules:         Option<Vec<InlineRule>>,
    /// Every other key, each a string, integer, float or boolean.
    params:        BTreeMap<String, String>,
}

/// A single forbidden-imports rule in inline array format.
struct InlineRule {
    scope:     String,
    forbidden: String,
    reason:    String,
    enabled:   bool,
}

/// The one refusal for a value of a type its key does not take. `found` is the
/// value as the TOML parser typed it, so the message says what the file holds
/// as well as what would have been read.
fn refuse_lint_type(
    source: &Path,
    lint_name: &str,
    key: &str,
    found: &toml_edit::Value,
    accepts: &str,
) -> ! {
    refuse_lint_entry(
        source,
        lint_name,
        &format!("`{key}` is {}, and it takes {accepts}", found.type_name()),
    )
}

const ACCEPTS_STRING: &str = "a string";
const ACCEPTS_STRINGS: &str = "an array of strings";
const ACCEPTS_SCALAR: &str = "a string, integer, float or boolean";

fn lint_string(source: &Path, lint: &str, key: &str, v: &toml_edit::Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => refuse_lint_type(source, lint, key, v, ACCEPTS_STRING),
    }
}

fn lint_strings(source: &Path, lint: &str, key: &str, v: &toml_edit::Value) -> Vec<String> {
    let Some(array) = v.as_array() else {
        refuse_lint_type(source, lint, key, v, ACCEPTS_STRINGS)
    };
    array
        .iter()
        .enumerate()
        .map(|(i, item)| lint_string(source, lint, &format!("{key}[{i}]"), item))
        .collect()
}

/// A param value as the string the lints read: a string, integer, float or boolean.
fn lint_scalar(source: &Path, lint: &str, key: &str, v: &toml_edit::Value) -> String {
    use toml_edit::Value;
    match v {
        Value::String(s) => s.value().clone(),
        Value::Integer(i) => i.value().to_string(),
        Value::Float(f) => f.value().to_string(),
        Value::Boolean(b) => b.value().to_string(),
        other => refuse_lint_type(source, lint, key, other, ACCEPTS_SCALAR),
    }
}

fn lint_table<'a>(
    source: &Path,
    lint: &str,
    key: &str,
    v: &'a toml_edit::Value,
    accepts: &str,
) -> &'a toml_edit::InlineTable {
    match v.as_inline_table() {
        Some(t) => t,
        None => refuse_lint_type(source, lint, key, v, accepts),
    }
}

fn lint_inline_rule(source: &Path, lint: &str, at: &str, v: &toml_edit::Value) -> InlineRule {
    let table = lint_table(
        source,
        lint,
        at,
        v,
        "a table with `scope`, `forbidden` and `reason`",
    );
    let required = |field: &str| -> String {
        let key = format!("{at}.{field}");
        match table.get(field) {
            Some(v) => lint_string(source, lint, &key, v),
            None => {
                refuse_lint_entry(
                    source,
                    lint,
                    &format!("`{key}` is missing, and it takes {ACCEPTS_STRING}"),
                )
            },
        }
    };
    let enabled = match table.get("enabled") {
        None => true,
        Some(v) => {
            v.as_bool().unwrap_or_else(|| {
                refuse_lint_type(source, lint, &format!("{at}.enabled"), v, "a boolean")
            })
        },
    };
    InlineRule {
        scope: required("scope"),
        forbidden: required("forbidden"),
        reason: required("reason"),
        enabled,
    }
}

/// Read a lint's inline table one key at a time, refusing the first value whose
/// type its key does not take.
fn read_lint_table(source: &Path, lint: &str, table: &toml_edit::InlineTable) -> LintTableConfig {
    let mut cfg = LintTableConfig::default();
    for (key, v) in table.iter() {
        match key {
            "commit" => cfg.commit = Some(lint_string(source, lint, key, v)),
            "build" => cfg.build = Some(lint_string(source, lint, key, v)),
            "push" => cfg.push = Some(lint_string(source, lint, key, v)),
            "severity" => cfg.severity = Some(lint_string(source, lint, key, v)),
            "include" => cfg.include = Some(lint_strings(source, lint, key, v)),
            "exclude" => cfg.exclude = Some(lint_strings(source, lint, key, v)),
            "crates" => cfg.crates = Some(lint_strings(source, lint, key, v)),
            "exempt_crates" => cfg.exempt_crates = Some(lint_strings(source, lint, key, v)),
            "findings" => {
                let t = lint_table(
                    source,
                    lint,
                    key,
                    v,
                    "a table of finding name to severity name",
                );
                cfg.findings = Some(
                    t.iter()
                        .map(|(kind, v)| {
                            let at = format!("findings.{kind}");
                            (kind.to_string(), lint_string(source, lint, &at, v))
                        })
                        .collect(),
                );
            },
            "rule" => {
                let rules = lint_table(source, lint, key, v, "a table of rule name to table");
                cfg.rule = Some(
                    rules
                        .iter()
                        .map(|(rule, v)| {
                            let at = format!("rule.{rule}");
                            let fields = lint_table(source, lint, &at, v, "a table of values");
                            let fields = fields
                                .iter()
                                .map(|(k, v)| {
                                    let at = format!("rule.{rule}.{k}");
                                    (k.to_string(), lint_scalar(source, lint, &at, v))
                                })
                                .collect();
                            (rule.to_string(), fields)
                        })
                        .collect(),
                );
            },
            "rules" => {
                let Some(array) = v.as_array() else {
                    refuse_lint_type(source, lint, key, v, "an array of tables")
                };
                cfg.rules = Some(
                    array
                        .iter()
                        .enumerate()
                        .map(|(i, v)| lint_inline_rule(source, lint, &format!("rules[{i}]"), v))
                        .collect(),
                );
            },
            _ => {
                cfg.params
                    .insert(key.to_string(), lint_scalar(source, lint, key, v));
            },
        }
    }
    cfg
}

// ---------------------------------------------------------------------------
// Config construction
// ---------------------------------------------------------------------------

impl Config {
    #[must_use]
    pub fn from_dir(mock_dir: &Path) -> Self {
        let mock_dir = mock_dir.to_path_buf();

        let repo_root = find_project_root(&mock_dir).unwrap_or_else(|| mock_dir.clone());
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

        // Not `unwrap_or_default`. A config that is present and does not parse
        // is a config the run never read, and taking the defaults for it means
        // every key the author wrote silently governs nothing: no namespaces,
        // no declared roots, no lint configuration, and a green run over all of
        // it. An absent config is the different case and stays free, since a
        // project that keeps none is ordinary.
        let raw: RawConfig = match toml_edit::de::from_str(&toml_content) {
            Ok(raw) => raw,
            Err(e) => {
                assert!(
                    toml_content.trim().is_empty(),
                    "mockspace.toml at {} does not parse, so nothing in it was \
                     read: {e}\n  Every key it declares would default instead, \
                     which is a run that reports success over a configuration it \
                     never opened.",
                    toml_path.display(),
                );
                RawConfig::default()
            },
        };

        let project_name = raw.project_name.unwrap_or_else(|| "project".into());
        let crate_prefix = raw.crate_prefix.unwrap_or_else(|| project_name.clone());

        // An entry is refused unless it is written the way git reports a path:
        // plain components joined by `/`, relative to the mock directory. The
        // entry is used verbatim as a path under the mock directory, which is how
        // the generated hooks and the phase gates address a directory, and any
        // other spelling matches no git path. Kept, the source under it would be
        // scoped to `infra` with nothing saying so, and `.` or an empty entry
        // would make discovery treat every directory under the mock dir as a
        // package. Refusing here, rather than folding the spelling in `SrcLayout`,
        // keeps discovery, the renderers and the hooks reading one string.
        let misspelt: Vec<String> = raw
            .src_dirs
            .iter()
            .filter_map(|d| {
                mockspace_lint_rules::src_layout::src_dir_fault(d)
                    .map(|why| format!("{d:?} ({why})"))
            })
            .collect();
        assert!(
            misspelt.is_empty(),
            "mockspace.toml names src_dirs that are not a plain relative path: {}\n  \
             Every entry is relative to the mock dir ({}) and is written as the \
             path of a directory under it, with plain components joined by `/`. \
             The hooks and the phase gates match a directory by that path, and \
             any other spelling matches none, so the entry would guard nothing. \
             Write it that way, and move the packages under the mock dir if they \
             are outside it.",
            misspelt.join(", "),
            mock_dir.display(),
        );
        // A declared list rather than a walked tree, because a walk cannot tell
        // an empty group from a directory that is not a group, and this project
        // has already shipped one silent-empty discovery.
        let src_dirs: Vec<PathBuf> = if raw.src_dirs.is_empty() {
            vec![mock_dir.join("crates")]
        } else {
            raw.src_dirs.iter().map(|d| mock_dir.join(d)).collect()
        };
        // A named directory that is missing is a configuration error and says
        // so; an absent subdirectory in a walk says nothing at all.
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

        // Refused like a missing `src_dirs` entry: a heavy-test threshold that
        // cannot mean anything would make every suite look broken.
        let test = raw
            .test
            .unwrap_or_default()
            .resolve()
            .unwrap_or_else(|e| panic!("mockspace.toml: {e}"));

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

        let lint_overrides = parse_lints_from_document(&toml_content, &crate_prefix, &toml_path);

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

        let mut renamed_keys = Vec::new();
        if let Some(complaint) = renamed_agent_key(raw_agent.agent_tool_catalogue.is_some()) {
            eprintln!("{complaint}");
            renamed_keys.push(complaint);
        }

        let agent = AgentConfig {
            renamed_keys,
            attribution,
            accelerated_interactive_design_talks: raw_agent
                .agent_accelerated_interactive_design_talks
                .unwrap_or(false),
            sketching_skill: raw_agent.agent_sketching_skill.unwrap_or(true),
            benchmarking_skill: raw_agent.agent_benchmarking_skill.unwrap_or(true),
            catalogues: raw_agent.agent_catalogues.unwrap_or(true),
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
        let auto_fmt = clone_says(&repo_root, "autoFmt")
            .or(raw.auto_fmt)
            .unwrap_or(true);
        let auto_clippy_fix = clone_says(&repo_root, "autoClippyFix")
            .or(raw.auto_clippy_fix)
            .unwrap_or(true);
        Config {
            mock_dir,
            crates_dir,
            src_dirs,
            append_only: raw.append_only.clone(),
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
            nuke_marker: DEFAULT_NUKE_MARKER.to_string(),
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
            auto_fmt,
            auto_clippy_fix,
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
            test,
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
/// Stop the load over a lint entry that cannot be read.
///
/// `Config::from_dir` has no `Result` to return and every other configuration
/// error in it (an unparsable file, a missing `src_dirs` entry, a bad `test`
/// block) is a panic naming the file, so this is one too, in the same channel.
/// The entry used to be skipped or defaulted, which dropped the lint's table
/// (severity, params, path and crate filters) or switched the lint off over one
/// wrong value, with nothing said.
fn refuse_lint_entry(source: &Path, lint_name: &str, why: &dyn std::fmt::Display) -> ! {
    panic!(
        "{}: [lints.{lint_name}] was not read: {}\n  \
         The lint would run on its defaults instead, which is a run that reports success over \
         a configuration it never applied. Fix the entry or remove it.",
        source.display(),
        why.to_string().trim_end(),
    )
}

/// The same refusal for a value that parsed as TOML and is not a name the engine
/// knows. `accepted` is the table the parser itself reads, so the list in the
/// message is the set that loads.
fn refuse_lint_value(
    source: &Path,
    lint_name: &str,
    subject: &str,
    value: &str,
    accepted: &[&str],
) -> ! {
    refuse_lint_entry(
        source,
        lint_name,
        &format!(
            "{subject} is \"{value}\", which is not a name it knows. Accepted: {}",
            accepted.join(", ")
        ),
    )
}

/// A gate key (`commit`, `build`, `push`) as the level it names, or the load stops.
fn gate_level(source: &Path, lint_name: &str, key: &str, value: &Option<String>) -> Option<Level> {
    value.as_deref().map(|s| {
        Level::from_str_name(s).unwrap_or_else(|| {
            let accepted: Vec<&str> = Level::NAMES.iter().map(|(name, _)| *name).collect();
            refuse_lint_value(source, lint_name, &format!("`{key}`"), s, &accepted)
        })
    })
}

/// A severity preset as the value it names, or the load stops. `subject` is how the
/// refusal names the value: a backticked key, or `the value` for an inline preset,
/// which has no key to name.
fn severity_preset(source: &Path, lint_name: &str, subject: &str, value: &str) -> Severity {
    parse_severity(value).unwrap_or_else(|| {
        let accepted: Vec<&str> = SEVERITY_NAMES.iter().map(|(name, _)| *name).collect();
        refuse_lint_value(source, lint_name, subject, value, &accepted)
    })
}

/// Parse the `[lints]` section from the TOML content using `toml_edit`'s
/// document API to handle heterogeneous values (strings and tables).
///
/// Every entry is turned into a `toml_edit::Value` and read by `read_lint_table`, so a
/// type error is reported against the key and never against line and column numbers
/// of a document that is not the file on disk.
/// Nothing in here drops or defaults a value it cannot read: each such arm goes
/// through `refuse_lint_entry`.
fn parse_lints_from_document(toml_content: &str, crate_prefix: &str, source: &Path) -> LintConfig {
    let doc = match toml_content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => panic!("{}: the file does not parse: {e}", source.display()),
    };

    let lints_item = match doc.get("lints") {
        Some(item) => item,
        None => return LintConfig::empty(),
    };

    // `[lints]` and `lints = { ... }` are both a table.
    let lints_table = match lints_item.as_table_like() {
        Some(t) => t,
        None => {
            panic!(
                "{}: `lints` is of type {}, and it has to be a table of lint names. Nothing under it was \
             read, so every lint would run on its defaults.",
                source.display(),
                lints_item.type_name(),
            )
        },
    };

    let mut base = HashMap::new();
    let mut findings: HashMap<String, HashMap<String, Severity>> = HashMap::new();
    let mut params: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut paths = PathFilters::new();
    let mut crates = CrateFilters::new();

    for (lint_name, item) in lints_table.iter() {
        let lint_name = lint_name.to_string();

        // Every form is read as a `Value`: a `[lints.name]` table becomes the
        // equivalent inline table, so one reader serves both and a refusal names
        // the key rather than a position in a document that is not the file.
        let value = match item {
            toml_edit::Item::Value(v) => v.clone(),
            toml_edit::Item::Table(t) => {
                toml_edit::Value::InlineTable(t.clone().into_inline_table())
            },
            toml_edit::Item::ArrayOfTables(_) => {
                refuse_lint_entry(
                    source,
                    &lint_name,
                    &"it is an array of tables ([[lints.name]]), and a lint is one table or one name",
                )
            },
            toml_edit::Item::None => refuse_lint_entry(source, &lint_name, &"it has no value"),
        };
        let entry = match value {
            toml_edit::Value::String(s) => LintEntry::Preset(s.into_value()),
            toml_edit::Value::InlineTable(ref table) => {
                LintEntry::Config(read_lint_table(source, &lint_name, table))
            },
            other => {
                refuse_lint_entry(
                    source,
                    &lint_name,
                    &format!(
                        "it is {}, and a lint is a severity name or a table",
                        other.type_name()
                    ),
                )
            },
        };

        match entry {
            LintEntry::Preset(s) => {
                let severity = severity_preset(source, &lint_name, "the value", &s);
                base.insert(lint_name, severity);
            },
            LintEntry::Config(table) => {
                // Determine base severity from "severity" key
                if let Some(ref s) = table.severity {
                    let severity = severity_preset(source, &lint_name, "`severity`", s);
                    base.insert(lint_name.clone(), severity);
                }

                // Per-gate severity from commit/build/push keys. Every key is
                // read before the lint gets an entry, so a bad one cannot leave
                // behind an all-`Pass` severity that switches the lint off.
                let commit = gate_level(source, &lint_name, "commit", &table.commit);
                let build = gate_level(source, &lint_name, "build", &table.build);
                let push = gate_level(source, &lint_name, "push", &table.push);
                if commit.is_some() || build.is_some() || push.is_some() {
                    let entry = base.entry(lint_name.clone()).or_insert(Severity::OFF);
                    if let Some(level) = commit {
                        entry.on_commit = level;
                    }
                    if let Some(level) = build {
                        entry.on_build = level;
                    }
                    if let Some(level) = push {
                        entry.on_push = level;
                    }
                }

                // Per-finding severity overrides
                if let Some(finding_map) = table.findings {
                    let entry = findings.entry(lint_name.clone()).or_default();
                    for (kind, val) in finding_map {
                        let key = format!("findings.{kind}");
                        entry.insert(
                            kind,
                            severity_preset(source, &lint_name, &format!("`{key}`"), &val),
                        );
                    }
                }

                // Path scoping. `{prefix}` expands here the same way it does in params, so
                // a shared lint pack can name a project's own crates in a glob.
                if table.include.is_some() || table.exclude.is_some() {
                    let expand = |v: Vec<String>| -> Vec<String> {
                        v.into_iter()
                            .map(|s| s.replace("{prefix}", crate_prefix))
                            .collect()
                    };
                    paths.insert(lint_name.clone(), PathFilter {
                        include: expand(table.include.unwrap_or_default()),
                        exclude: expand(table.exclude.unwrap_or_default()),
                    });
                }

                if table.crates.is_some() || table.exempt_crates.is_some() {
                    let expand = |v: Vec<String>| -> Vec<String> {
                        v.into_iter()
                            .map(|s| s.replace("{prefix}", crate_prefix))
                            .collect()
                    };
                    crates.insert(lint_name.clone(), PathFilter {
                        include: expand(table.crates.unwrap_or_default()),
                        exclude: expand(table.exempt_crates.unwrap_or_default()),
                    });
                }

                // Named rule sub-tables: [lints.lint-name.rule.rule-name]
                if let Some(rule_map) = table.rule {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (rule_name, rule_fields) in rule_map {
                        for (key, val) in rule_fields {
                            let val_str = val.replace("{prefix}", crate_prefix);
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
                        let val_str = val.replace("{prefix}", crate_prefix);
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
        crates,
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// The project root: where the config sits and what `docs/` hangs off.
///
/// `.git` first, because in a clone that is the root by definition and it is
/// the marker every other part of the tool already agrees on. `mockspace.toml`
/// second, because **a tree with no `.git` is ordinary**: an export, a tarball,
/// a vendored copy, an rsync that excluded the admin directory, a build
/// container handed the sources and not the history.
///
/// Without the second marker such a tree finds no root, falls back to the mock
/// dir, looks for the config inside it, does not find it there either, and
/// reads the empty string. Every key then takes its default and nothing is
/// said. Measured on one repo: a clone answers `46 rows across 1 namespaces`,
/// and a copy of the same tree with `.git` excluded answers `0 rows across 0
/// namespaces`, naming the two namespaces the engine supplies itself.
///
/// The order is what keeps this additive: a tree that has `.git` resolves
/// exactly as it did before, so the second marker is reached only where the
/// answer used to be the mock dir.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    // Two passes over the ancestry rather than one checking both markers at
    // each level, so a `.git` further up still wins over a `mockspace.toml`
    // nearer. That is what makes the change additive: the second pass runs
    // only where the first returned nothing at all.
    find_upward(start, ".git").or_else(|| find_upward(start, "mockspace.toml"))
}

/// Whether git reads `root` as the top of a repository of its own.
///
/// A `.git` git does not recognise, an empty directory or a stray file, answers
/// every `--local` read with the same fatal status a malformed value gets, so
/// without asking first a root with no clone behind it would be refused as a
/// clone holding a bad setting. And where such a root sits inside another
/// repository, git answers for that one instead, which is the surrounding
/// repository this reader is not supposed to ask; so the top git names has to
/// be `root` itself.
///
/// Only "not a repository" is read as no clone. Any other refusal, a `.git`
/// file pointing nowhere or a clone git will not read for its ownership, is a
/// clone whose setting cannot be read, and that panics like a malformed value
/// does rather than letting the switch quietly do nothing. The message is
/// matched with the locale pinned, since git translates it.
fn git_answers_for(root: &Path) -> bool {
    let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .env("LC_ALL", "C")
        .current_dir(root)
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("not a git repository") {
            return false;
        }
        panic!(
            "git cannot read the clone at {}: {}",
            root.display(),
            stderr.trim()
        );
    }
    let top = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    match (top.canonicalize(), root.canonicalize()) {
        (Ok(top), Ok(root)) => top == root,
        _ => false,
    }
}

/// What this clone says about one of the commit fixers, read from `git config`
/// under `mockspace.<key>`, or nothing where it says nothing.
///
/// `mockspace.toml` is the project's and travels with every clone of it. This
/// setting is the clone's own, so a machine that should not compile the project
/// at commit time can turn the fixer off for itself without the project
/// carrying which machine that is. Where both are set, the clone's wins.
///
/// Read only where the root is a git tree. A root found by `mockspace.toml`
/// alone has no clone to ask, and asking git from there would read whatever
/// repository happens to sit around it. A value git cannot read as a boolean
/// is refused rather than passed over, since a switch that silently does
/// nothing is the thing a person set it to prevent.
///
/// `--local`, so the global and system files are not read. A setting in
/// `~/.gitconfig` would reach every clone on the machine, which is the
/// project-wide answer this switch exists to not be. A worktree has a `.git`
/// file rather than a directory and reads its clone's config, so it gets the
/// clone's answer.
fn clone_says(repo_root: &Path, key: &str) -> Option<bool> {
    if !repo_root.join(".git").exists() || !git_answers_for(repo_root) {
        return None;
    }
    let name = format!("mockspace.{key}");
    let out = std::process::Command::new("git")
        .args(["config", "--local", "--type=bool", "--get", &name])
        .current_dir(repo_root)
        .output()
        .ok()?;
    // 1 is git's answer for a key that is not set.
    if out.status.code() == Some(1) {
        return None;
    }
    if !out.status.success() {
        panic!(
            "`{name}` in this clone's git config does not parse as a boolean: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        other => panic!("`{name}` in this clone's git config reads as `{other}`, not a boolean"),
    }
}

fn find_upward(start: &Path, marker: &str) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(marker).exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod a_renamed_key_is_not_dropped_in_silence {
    //! `#[serde(default)]` with no `deny_unknown_fields` drops an unknown key
    //! without a word. For a key that used to mean something that is not a
    //! tidiness matter: the consumer wrote `false` to keep builtin prose out of
    //! their agent context, the key stops being read, the rule defaults on, and
    //! the prose lands anyway with nothing said. The decision to complain is
    //! split out of the loader precisely so this can assert it.

    use super::*;

    #[test]
    fn the_old_key_is_named_along_with_what_replaced_it() {
        let complaint = renamed_agent_key(true).expect("the old key must not pass in silence");
        assert!(
            complaint.contains("agent_tool_catalogue"),
            "a reader has to find the line they wrote: {complaint}"
        );
        assert!(
            complaint.contains("agent_catalogues"),
            "and be told what to write instead: {complaint}"
        );
        assert!(
            complaint.contains("NOT read"),
            "and that the old one governs nothing, or they will assume it still works: \
             {complaint}"
        );
    }

    /// The control. Without it the function could return the complaint always.
    #[test]
    fn a_config_that_does_not_use_the_old_key_is_not_complained_at() {
        assert!(renamed_agent_key(false).is_none());
    }

    #[test]
    fn crates_and_exempt_crates_under_a_lint_become_its_crate_filter() {
        let toml = r#"
[lints.no-std]
commit = "error"
crates = ["{prefix}-dirs", "{prefix}-config"]
exempt_crates = ["{prefix}"]

[lints.no-alloc]
commit = "error"
exempt_crates = ["widget"]

[lints.file-size]
commit = "error"
"#;
        let cfg = parse_lints_from_document(toml, "widget", Path::new("mockspace.toml"));

        assert!(cfg.binds_crate("no-std", "widget-dirs"));
        assert!(cfg.binds_crate("no-std", "widget-config"));
        assert!(
            !cfg.binds_crate("no-std", "widget"),
            "the launcher is exempt"
        );
        assert!(
            !cfg.binds_crate("no-std", "unrelated"),
            "a `crates` list admits only what it names"
        );

        assert!(!cfg.binds_crate("no-alloc", "widget"));
        assert!(
            cfg.binds_crate("no-alloc", "widget-dirs"),
            "an exemption alone admits everything else"
        );

        // the control: a lint naming neither key binds every crate, and is not
        // in the map at all
        assert!(!cfg.crates.contains_key("file-size"));
        assert!(cfg.binds_crate("file-size", "widget"));
        assert!(cfg.binds_crate("never-mentioned", "widget"));
    }

    /// The old key is captured rather than passing as an unknown field.
    ///
    /// This is the half that actually breaks if somebody deletes the field as
    /// dead code: serde would drop it silently again and the complaint above
    /// would never fire, while both arms of that test still pass.
    #[test]
    fn the_raw_config_still_captures_the_old_key_so_it_can_be_refused() {
        let raw: RawAgentConfig =
            toml_edit::de::from_str("agent_tool_catalogue = false").expect("parses");
        assert!(
            raw.agent_tool_catalogue.is_some(),
            "the field exists only to notice the key; without it the rename is silent"
        );
        assert!(
            renamed_agent_key(raw.agent_tool_catalogue.is_some()).is_some(),
            "and noticing it has to reach the complaint"
        );
    }

    /// A `mock` dir carrying an `agent/config.toml` with the given body.
    fn mock_dir_with_agent_config(body: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = tmp.path().join("agent");
        std::fs::create_dir_all(&agent).expect("create agent dir");
        std::fs::write(agent.join("config.toml"), body).expect("write agent config");
        tmp
    }

    /// The load itself notices, which is the half nothing held.
    ///
    /// The three arms above all call `renamed_agent_key` themselves, so every
    /// one of them passes with the call site deleted from `from_dir` and the
    /// old key going through in silence again. Measured: commenting that block
    /// out left the suite green at 26 of 26. This is the arm that goes red.
    #[test]
    fn a_load_records_the_complaint_it_printed() {
        let tmp = mock_dir_with_agent_config("agent_tool_catalogue = false\n");
        let cfg = Config::from_dir(tmp.path());
        assert_eq!(
            cfg.agent.renamed_keys.len(),
            1,
            "a load that read the old key has one thing to say about it"
        );
        let complaint = &cfg.agent.renamed_keys[0];
        assert!(
            complaint.contains("agent_tool_catalogue") && complaint.contains("agent_catalogues"),
            "and it is the complaint naming both spellings: {complaint}"
        );
    }

    /// The control. Without it an implementation pushing unconditionally, or
    /// one seeding the field with a line of text, satisfies the arm above.
    #[test]
    fn a_load_of_a_config_spelling_the_new_key_records_nothing() {
        let tmp = mock_dir_with_agent_config("agent_catalogues = false\n");
        let cfg = Config::from_dir(tmp.path());
        assert!(
            cfg.agent.renamed_keys.is_empty(),
            "nothing was renamed, so there is nothing to say: {:?}",
            cfg.agent.renamed_keys
        );
        assert!(
            !cfg.agent.catalogues,
            "control on the control: the new key is read, so this config is one \
             the loader genuinely understood rather than one it failed to open"
        );
    }
}

#[cfg(test)]
mod a_malformed_lint_table_is_not_dropped_in_silence {
    //! A `[lints.<name>]` table holding a value of the wrong type used to be
    //! skipped, so one wrong type (a string where `exclude` wants an array) turned
    //! the whole lint's configuration off, severity and params included, with
    //! nothing said. The load now refuses it, in the way every other config error
    //! in `from_dir` is refused, and names the lint, the file and the key at fault.

    use super::*;

    /// A `mock` dir carrying a `mockspace.toml` with the given body.
    fn mock_dir_with_config(body: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("mockspace.toml"), body).expect("write config");
        tmp
    }

    /// The panic text of a load, or `None` when the load went through.
    fn refusal_of(body: &str) -> Option<String> {
        let tmp = mock_dir_with_config(body);
        let dir = tmp.path().to_path_buf();
        let caught = std::panic::catch_unwind(move || {
            let _ = Config::from_dir(&dir);
        });
        caught.err().map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default()
        })
    }

    #[test]
    fn a_well_formed_lint_table_loads_with_its_severity_and_path_filter() {
        let tmp = mock_dir_with_config(
            "[lints.catalogue-ignore]\nseverity = \"error\"\nexclude = [\"fixtures/**\"]\n",
        );
        let cfg = Config::from_dir(tmp.path());
        assert_eq!(
            cfg.lint_overrides.base.get("catalogue-ignore"),
            Some(&Severity::HARD_ERROR),
            "the severity the table wrote has to reach the config as the value it named"
        );
        assert_eq!(
            cfg.lint_overrides.filter_for("catalogue-ignore"),
            Some(&PathFilter {
                include: Vec::new(),
                exclude: vec!["fixtures/**".to_string()],
            }),
            "and so does the path filter, or the arm above passes on a table read as empty"
        );
    }

    /// What a load made of one lint's severity, for the failure message of a
    /// test that expected a refusal and got a quiet load instead.
    fn loaded_severity(body: &str, lint: &str) -> String {
        let tmp = mock_dir_with_config(body);
        let cfg = Config::from_dir(tmp.path());
        format!("{:?}", cfg.lint_overrides.base.get(lint))
    }

    /// Asserts the load stops, and that the message names the lint, the file,
    /// the offending value and the accepted ones, so the fix is in the message.
    fn assert_refused_naming(body: &str, lint: &str, must_name: &[&str]) -> String {
        let msg = refusal_of(body).unwrap_or_else(|| {
            panic!(
                "a bad value must stop the load, but it loaded and the lint became {}",
                loaded_severity(body, lint)
            )
        });
        assert!(msg.contains(lint), "names the lint: {msg}");
        assert!(msg.contains("mockspace.toml"), "names the file: {msg}");
        for needle in must_name {
            assert!(msg.contains(needle), "names `{needle}`: {msg}");
        }
        msg
    }

    /// The accepted severity spellings, read from the table `parse_severity` reads. Not a
    /// copy: a copy still passes after a spelling is deleted from the table.
    fn severity_words() -> Vec<&'static str> {
        SEVERITY_NAMES.iter().map(|(name, _)| *name).collect()
    }

    fn gate_words() -> Vec<&'static str> {
        Level::NAMES.iter().map(|(name, _)| *name).collect()
    }

    #[test]
    fn a_misspelt_inline_preset_is_refused_and_lists_the_accepted_values() {
        let msg = assert_refused_naming("[lints]\nfile-size = \"eror\"\n", "file-size", &["eror"]);
        for word in severity_words() {
            assert!(msg.contains(word), "lists `{word}`: {msg}");
        }
        assert!(
            !msg.contains("`the value`"),
            "an inline preset has no key to name, so none is backticked: {msg}"
        );
    }

    #[test]
    fn a_misspelt_severity_key_is_refused_and_names_the_key() {
        assert_refused_naming("[lints.file-size]\nseverity = \"eror\"\n", "file-size", &[
            "severity", "eror",
        ]);
    }

    #[test]
    fn a_misspelt_finding_severity_is_refused_and_names_the_finding() {
        assert_refused_naming(
            "[lints.file-size]\ncommit = \"error\"\n[lints.file-size.findings]\n\
             too-long = \"eror\"\n",
            "file-size",
            &["findings", "too-long", "eror"],
        );
    }

    #[test]
    fn a_misspelt_gate_level_is_refused_for_each_of_the_three_gates() {
        for gate in ["commit", "build", "push"] {
            let msg = assert_refused_naming(
                &format!("[lints.file-size]\n{gate} = \"eror\"\n"),
                "file-size",
                &[gate, "eror"],
            );
            for word in gate_words() {
                assert!(msg.contains(word), "lists the gate level `{word}`: {msg}");
            }
        }
    }

    /// The control for the gate arm: every gate level a consumer writes loads
    /// to the level it names, so the refusal is not a refusal of the whole key.
    #[test]
    fn the_gate_levels_in_use_load_to_the_level_they_name() {
        let tmp = mock_dir_with_config(
            "[lints.file-size]\ncommit = \"off\"\nbuild = \"warn\"\npush = \"error\"\n",
        );
        let cfg = Config::from_dir(tmp.path());
        assert_eq!(
            cfg.lint_overrides.base.get("file-size"),
            Some(&Severity::new(Level::Pass, Level::Warn, Level::Error))
        );
    }

    /// The words a sentence of the usage guide puts in backticks, in order.
    fn backticked(text: &str) -> Vec<String> {
        text.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    }

    /// The guide lists what is accepted, and a list nobody checks against the tables is a
    /// comment with a type: it is read back here and compared to them.
    #[test]
    fn the_usage_guide_lists_every_spelling_the_tables_accept() {
        let guide = include_str!("../docs/USAGE_GUIDE.md");
        let from = guide
            .find("A severity takes ")
            .expect("the guide has to say what a severity takes");
        let rest = &guide[from ..];
        let (severity, gate) = rest
            .split_once("; a gate key takes ")
            .expect("and what a gate key takes, in the same sentence");
        let (gate, _) = gate.split_once(". ").expect("the sentence ends");

        let mut listed = backticked(severity);
        let mut accepted: Vec<String> = severity_words().iter().map(|s| s.to_string()).collect();
        listed.sort();
        accepted.sort();
        assert_eq!(
            listed, accepted,
            "the guide's severity list against `SEVERITY_NAMES`"
        );

        let mut listed = backticked(gate);
        let mut accepted: Vec<String> = gate_words().iter().map(|s| s.to_string()).collect();
        listed.sort();
        accepted.sort();
        assert_eq!(
            listed, accepted,
            "the guide's gate list against `Level::NAMES`"
        );
    }

    #[test]
    fn a_lints_key_that_is_not_a_table_is_refused() {
        let msg = refusal_of("lints = \"error\"\n")
            .expect("`lints = \"error\"` is a section that was never read, and must say so");
        assert!(msg.contains("mockspace.toml"), "names the file: {msg}");
        assert!(msg.contains("lints"), "names the key: {msg}");
    }

    #[test]
    fn a_lints_section_written_inline_is_read_rather_than_dropped() {
        let tmp = mock_dir_with_config("lints = { file-size = \"error\" }\n");
        let cfg = Config::from_dir(tmp.path());
        assert_eq!(
            cfg.lint_overrides.base.get("file-size"),
            Some(&Severity::HARD_ERROR),
            "an inline table is a table, and used to load as an empty config"
        );
    }

    #[test]
    fn an_array_of_tables_for_one_lint_is_refused() {
        assert_refused_naming("[[lints.file-size]]\ncommit = \"error\"\n", "file-size", &[
            "array of tables",
        ]);
    }

    #[test]
    fn a_refusal_carries_no_coordinates_into_a_document_that_is_not_the_file() {
        // `exclude` is on line 3 of the real file and line 2 of any fragment.
        let msg = assert_refused_naming(
            "# a comment\n[lints.file-size]\nexclude = \"x\"\n",
            "file-size",
            &["exclude"],
        );
        assert!(
            !msg.contains("line ") && !msg.contains("column"),
            "a line number here would point into a document that is not on disk: {msg}"
        );
    }

    #[test]
    fn an_inline_table_with_a_wrong_type_names_the_field_not_the_untagged_enum() {
        let msg = assert_refused_naming(
            "[lints]\nfile-size = { exclude = \"x\" }\n",
            "file-size",
            &["exclude"],
        );
        assert!(
            !msg.contains("untagged"),
            "serde's own wording is no help: {msg}"
        );
    }

    /// A value of the wrong shape is refused against the key it sits under and
    /// never in serde's words for an enum nobody sees.
    fn assert_refused_at_key(body: &str, key: &str) {
        let msg = assert_refused_naming(body, "file-size", &[key]);
        assert!(
            !msg.contains("untagged"),
            "serde's own wording is no help: {msg}"
        );
        assert!(
            !msg.contains("data did not match"),
            "nor is this one: {msg}"
        );
        assert!(
            !msg.contains("StringOrOther"),
            "nor the type behind it: {msg}"
        );
    }

    #[test]
    fn an_array_for_a_param_is_refused_and_names_the_param() {
        assert_refused_at_key("[lints.file-size]\nmax_lines = [1, 2]\n", "max_lines");
    }

    #[test]
    fn a_table_for_a_param_is_refused_and_names_the_param() {
        assert_refused_at_key("[lints.file-size]\nmax_lines = { a = 1 }\n", "max_lines");
    }

    #[test]
    fn a_non_string_finding_severity_is_refused_and_names_the_finding() {
        assert_refused_at_key("[lints.file-size]\nfindings = { a = 5 }\n", "findings.a");
    }

    #[test]
    fn findings_that_is_not_a_table_is_refused_and_names_findings() {
        assert_refused_at_key("[lints.file-size]\nfindings = \"error\"\n", "findings");
    }

    #[test]
    fn a_rule_value_of_the_wrong_type_is_refused_and_names_the_key() {
        assert_refused_at_key(
            "[lints.file-size.rule.core]\nscope = [\"a\"]\n",
            "rule.core.scope",
        );
    }

    #[test]
    fn a_rule_that_is_not_a_table_is_refused_and_names_the_rule() {
        assert_refused_at_key("[lints.file-size.rule]\ncore = \"x\"\n", "rule.core");
    }

    #[test]
    fn an_inline_rule_with_no_reason_is_refused_and_names_the_missing_key() {
        assert_refused_at_key(
            "[lints.file-size]\nrules = [{ scope = \"a\", forbidden = \"b\" }]\n",
            "rules[0].reason",
        );
    }

    #[test]
    fn an_inline_rule_value_of_the_wrong_type_is_refused_and_names_the_key() {
        assert_refused_at_key(
            "[lints.file-size]\nrules = [{ scope = \"a\", forbidden = 3, reason = \"c\" }]\n",
            "rules[0].forbidden",
        );
        assert_refused_at_key(
            "[lints.file-size]\nrules = [{ scope = \"a\", forbidden = \"b\", reason = \"c\", \
             enabled = \"no\" }]\n",
            "rules[0].enabled",
        );
    }

    #[test]
    fn rules_that_is_not_an_array_of_tables_is_refused_and_names_rules() {
        assert_refused_at_key("[lints.file-size]\nrules = \"x\"\n", "rules");
        assert_refused_at_key("[lints.file-size]\nrules = [\"x\"]\n", "rules[0]");
    }

    #[test]
    fn a_scalar_key_of_the_wrong_type_is_refused_and_names_the_key() {
        for key in ["severity", "commit", "build", "push"] {
            assert_refused_at_key(&format!("[lints.file-size]\n{key} = 5\n"), key);
        }
        assert_refused_at_key("[lints.file-size]\ninclude = [1]\n", "include");
        assert_refused_at_key("[lints.file-size]\ncrates = \"x\"\n", "crates");
        assert_refused_at_key(
            "[lints.file-size]\nexempt_crates = [true]\n",
            "exempt_crates",
        );
    }

    /// The control: every shape the arms above refuse has a sibling that loads,
    /// so none of them is a refusal of the whole key.
    #[test]
    fn the_well_typed_siblings_of_those_shapes_load_to_what_they_say() {
        let tmp = mock_dir_with_config(
            "[lints.file-size]\nmax_lines = 400\nratio = 0.5\nstrict = true\nname = \"x\"\n\
             findings = { a = \"warn\" }\n\
             rules = [{ scope = \"s\", forbidden = \"f\", reason = \"r\" }, \
             { scope = \"off\", forbidden = \"f\", reason = \"r\", enabled = false }, \
             { scope = \"t\", forbidden = \"g\", reason = \"q\" }]\n\
             [lints.file-size.rule.core]\nscope = \"{prefix}-core\"\nlimit = 3\n",
        );
        let cfg = Config::from_dir(tmp.path());
        let params = &cfg.lint_overrides.params["file-size"];
        assert_eq!(params["max_lines"], "400");
        assert_eq!(params["ratio"], "0.5");
        assert_eq!(params["strict"], "true");
        assert_eq!(params["name"], "x");
        assert_eq!(params["rule.core.limit"], "3");
        assert_eq!(params["rule.core.scope"], "project-core");
        assert_eq!(params["rule.rule-0.scope"], "s");
        assert_eq!(params["rule.rule-0.reason"], "r");
        assert_eq!(params["rule.rule-2.scope"], "t");
        assert_eq!(params["rule.rule-2.forbidden"], "g");
        assert_eq!(params["rule.rule-2.reason"], "q");
        assert!(
            !params.keys().any(|k| k.starts_with("rule.rule-1.")),
            "a disabled rule is dropped, and its index is not reused: {params:?}"
        );
        assert_eq!(
            cfg.lint_overrides.findings["file-size"]["a"],
            Severity::ADVISORY
        );
    }

    /// The nested `[[lints.<name>.rules]]` spelling of the control's three entries,
    /// with a key the reader does not name (`note`) on the first. That key is
    /// tolerated, not refused, and carries nothing into the params.
    #[test]
    fn the_nested_rules_spelling_loads_the_same_entries_and_tolerates_an_extra_key() {
        let tmp = mock_dir_with_config(
            "[lints.file-size]\nmax_lines = 400\n\
             [[lints.file-size.rules]]\nscope = \"s\"\nforbidden = \"f\"\nreason = \"r\"\n\
             note = \"not read\"\n\
             [[lints.file-size.rules]]\nscope = \"off\"\nforbidden = \"f\"\nreason = \"r\"\n\
             enabled = false\n\
             [[lints.file-size.rules]]\nscope = \"t\"\nforbidden = \"g\"\nreason = \"q\"\n",
        );
        let cfg = Config::from_dir(tmp.path());
        let params = &cfg.lint_overrides.params["file-size"];
        assert_eq!(params["max_lines"], "400");
        assert_eq!(params["rule.rule-0.scope"], "s");
        assert_eq!(params["rule.rule-0.forbidden"], "f");
        assert_eq!(params["rule.rule-0.reason"], "r");
        assert_eq!(params["rule.rule-2.scope"], "t");
        assert_eq!(params["rule.rule-2.forbidden"], "g");
        assert_eq!(params["rule.rule-2.reason"], "q");
        assert!(
            !params.keys().any(|k| k.starts_with("rule.rule-1.")),
            "a disabled rule is dropped in the nested spelling too: {params:?}"
        );
        assert!(
            !params.keys().any(|k| k.contains("note")),
            "an unnamed key is not a param: {params:?}"
        );
    }

    #[test]
    fn a_string_where_exclude_wants_an_array_is_refused_and_names_the_lint() {
        let msg = refusal_of(
            "[lints.catalogue-ignore]\nseverity = \"error\"\nexclude = \"fixtures/**\"\n",
        )
        .expect("a lint table with a value of the wrong type must stop the load, not vanish");
        assert!(msg.contains("catalogue-ignore"), "names the lint: {msg}");
        assert!(msg.contains("mockspace.toml"), "names the file: {msg}");
        assert!(msg.contains("exclude"), "names the key at fault: {msg}");
    }

    #[test]
    fn an_inline_lint_value_of_the_wrong_type_is_refused_too() {
        let msg = refusal_of("[lints]\nfile-size = 5\n")
            .expect("the inline form has its own skip and gets the same refusal");
        assert!(msg.contains("file-size"), "names the lint: {msg}");
    }

    /// The control. Without it a loader that panicked on every config with a
    /// `[lints]` section would satisfy the arms above.
    #[test]
    fn a_config_with_no_malformed_lint_is_not_refused() {
        assert!(refusal_of("[lints.file-size]\ncommit = \"error\"\n").is_none());
        assert!(refusal_of("[lints]\nfile-size = \"error\"\n").is_none());
    }
}

#[cfg(test)]
#[path = "config_clone_says_tests.rs"]
mod clone_says_tests;

#[cfg(test)]
#[path = "config_src_dirs_tests.rs"]
mod src_dirs_tests;

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
            // `lints` and `lint-crates` are read through toml_edit rather than
            // through RawConfig, deliberately: the first because its values are
            // heterogeneous, the second because `bootstrap::lints` reads it
            // before a config is parsed at all. Both are named here rather than
            // covered by a rule, so the next one has to be named too instead of
            // slipping in behind a pattern.
            //
            // `lint-crates` was missing from ROOT_KEYS entirely, which made
            // every repo declaring an external lint pack report its own
            // declaration as discarded. This guard is what catches the opposite
            // mistake, and it fired the moment the key was added, which is the
            // guard working.
            if *k == "lints" || *k == "lint-crates" || launcher.iter().any(|l| l == k) {
                continue;
            }
            assert!(
                keys.iter().any(|e| e == k),
                "ROOT_KEYS names `{k}`, which RawConfig no longer carries"
            );
        }
    }
}
