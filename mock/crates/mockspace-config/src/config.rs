//! Canonical v2 `mockspace.toml` schema types (spec §46).
//!
//! The `Config` struct here is the root of a parsed `mockspace.toml`. Every
//! sub-section is named to match the spec exactly. Iteration over map-valued
//! sections is deterministic (`BTreeMap`) to satisfy the §47 determinism
//! requirement on rendered output.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Parsed `mockspace.toml`. See spec §46 for the full schema reference.
///
/// Schema is strict: every top-level field is enumerated below and any
/// other key in the TOML source surfaces as an unknown-field error at
/// parse time. Retired sections (e.g. the legacy
/// `[primitive-introductions]` table) fail here with no special-cased
/// detection; consumers get a specific serde error pointing at the
/// offending key.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub mockspace: MockspaceSection,
    #[serde(default)]
    pub refs: RefsSection,
    /// Name of the primary host (active forge integration target).
    /// Matches a key in [`Self::hosts`]. PRs, issues, and signing flow
    /// against this host; other hosts are import / mirror sources.
    #[serde(default)]
    pub primary_host: Option<String>,
    #[serde(default)]
    pub hosts: BTreeMap<String, HostSection>,
    #[serde(default)]
    pub imports: ImportsSection,
    #[serde(default, rename = "lint-crates")]
    pub lint_crates: BTreeMap<String, LintCrateRef>,
    #[serde(default)]
    pub lints: BTreeMap<String, LintConfig>,
    #[serde(default)]
    pub languages: BTreeMap<String, LanguageEntry>,
    #[serde(default)]
    pub profile: BTreeMap<String, ProfileSection>,
    #[serde(default)]
    pub crate_colors: BTreeMap<String, CrateColor>,
    #[serde(default)]
    pub domain_kinds: BTreeMap<String, DomainKind>,
    #[serde(default)]
    pub known_macros: BTreeMap<String, KnownMacro>,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub primary_domain_macro: Option<String>,
    #[serde(default)]
    pub primary_domain_label: Option<String>,
    #[serde(default)]
    pub transparency: TransparencySection,
    #[serde(default)]
    pub undo: UndoSection,
}

/// `[mockspace]` top block. See spec §46, §57 (`mock_bin_path`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MockspaceSection {
    /// Intended mockspace tool version (`major.minor`). Loader rejects on mismatch.
    pub version: String,
    #[serde(default = "default_profile")]
    pub default_profile: String,
    #[serde(default = "default_one_active_round")]
    pub default_one_active_round: bool,
    #[serde(default = "default_verifier_timeout_seconds")]
    pub verifier_timeout_seconds: u64,
    /// Step 2 in the invocation resolution chain (spec §57).
    ///
    /// Relative to the directory containing this `mockspace.toml`. Absolute
    /// paths trigger a portable-path warning at load (see §57 "Portable
    /// paths"). The resolver and warning helper live in `mockspace-rs`.
    #[serde(default)]
    pub mock_bin_path: Option<PathBuf>,
}

impl Default for MockspaceSection {
    fn default() -> Self {
        Self {
            version: default_version(),
            default_profile: default_profile(),
            default_one_active_round: default_one_active_round(),
            verifier_timeout_seconds: default_verifier_timeout_seconds(),
            mock_bin_path: None,
        }
    }
}

fn default_version() -> String {
    "1.0".into()
}
fn default_profile() -> String {
    "dev".into()
}
fn default_one_active_round() -> bool {
    true
}
fn default_verifier_timeout_seconds() -> u64 {
    30
}

/// `[refs]` block. Ref-storage policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefsSection {
    #[serde(default = "default_true")]
    pub mirror_ext_refs: bool,
    #[serde(default)]
    pub push_mirrors: bool,
    #[serde(default = "default_true")]
    pub fetch_on_reference: bool,
    #[serde(default = "default_task_archive_threshold_days")]
    pub task_archive_threshold_days: u32,
    #[serde(default = "default_round_archive_threshold_days")]
    pub round_archive_threshold_days: u32,
    #[serde(default)]
    pub security: RefsSecuritySection,
}

impl Default for RefsSection {
    fn default() -> Self {
        Self {
            mirror_ext_refs: true,
            push_mirrors: false,
            fetch_on_reference: true,
            task_archive_threshold_days: default_task_archive_threshold_days(),
            round_archive_threshold_days: default_round_archive_threshold_days(),
            security: RefsSecuritySection::default(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_task_archive_threshold_days() -> u32 {
    90
}
fn default_round_archive_threshold_days() -> u32 {
    365
}

/// `[refs.security]` block. Supply-chain controls on ref import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefsSecuritySection {
    /// Hostnames permitted as ref import sources. Glob-style entries like
    /// `*.example.com` allowed. Empty means no allowlist filtering.
    #[serde(default)]
    pub domain_allowlist: Vec<String>,
    #[serde(default = "default_true")]
    pub require_https: bool,
}

impl Default for RefsSecuritySection {
    fn default() -> Self {
        Self {
            domain_allowlist: Vec::new(),
            require_https: true,
        }
    }
}

/// `[hosts.<name>]` block. One schema for every host. The primary host
/// (named by `[Config::primary_host]`) fills the forge-integration
/// fields (`kind`, `token_env`, PR/auto-merge config, etc.); secondary
/// hosts typically only fill `url` (plus optionally `forge_url_template`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSection {
    pub url: String,
    /// Optional template for constructing web-UI URLs from refs, using the
    /// `{ref}` placeholder. Example: `"https://github.com/foo/bar/tree/{ref}"`.
    #[serde(default)]
    pub forge_url_template: Option<String>,
    /// Forge software type. Required when this host is the primary; ignored
    /// otherwise.
    #[serde(rename = "type", default)]
    pub kind: Option<ForgeKind>,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default)]
    pub auto_open_pr: Option<bool>,
    #[serde(default)]
    pub auto_push_body: Option<bool>,
    #[serde(default)]
    pub auto_merge_on_done: Option<bool>,
    #[serde(default)]
    pub merge_style: Option<MergeStyle>,
    #[serde(default)]
    pub default_base_branch: Option<String>,
    #[serde(default)]
    pub pr_body_managed_section_delimiter_start: Option<String>,
    #[serde(default)]
    pub pr_body_managed_section_delimiter_end: Option<String>,
    #[serde(default)]
    pub api_retry_attempts: Option<u32>,
    #[serde(default)]
    pub api_retry_backoff_seconds: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeKind {
    Github,
    Forgejo,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeStyle {
    #[default]
    Squash,
    Merge,
    Rebase,
}

/// `[imports]` block. Static ref imports and per-host extensions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportsSection {
    /// Flat list of `mock://...` imports to pull at load.
    ///
    /// Each entry is either a bare URI string (legacy form) or a
    /// structured `{ uri, kind }` table (per the preset infrastructure
    /// memo at `mock/research/202605220500_lint-preset-infrastructure.md`).
    /// The structured form names the trust tier under which the import
    /// resolves; the bare form defaults to the `executable` tier for
    /// backward compatibility with v1 imports.
    #[serde(default)]
    pub import: Vec<ImportEntry>,
    /// `[imports.ext.<host>]` entries. Per-host file-glob and runner config.
    #[serde(default)]
    pub ext: BTreeMap<String, ExtImport>,
}

/// One `[imports]` entry. Untagged: TOML parses a bare string into
/// [`ImportEntry::Uri`] and a `{ uri, kind = "..." }` table into
/// [`ImportEntry::Typed`].
///
/// The `kind` axis carries the trust tier so the loader applies the
/// right verification ceremony per the preset memo (`kind = "config"`
/// is SHA-pinned but NOT signature-verified; the default executable
/// kind is signature-verified + TOFU per spec §30).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImportEntry {
    /// Bare URI string. Defaults to [`ImportKind::Executable`] at
    /// resolution time. Equivalent to v1's flat-string import list.
    Uri(String),
    /// Structured entry with explicit kind.
    Typed(TypedImport),
}

/// Structured `{ uri = "...", kind = "..." }` import. The `kind` field
/// selects the trust tier; see [`ImportKind`].
///
/// `deny_unknown_fields` catches typos in the inline-table form (e.g.
/// `{ uri = "x", knd = "config" }`) which would otherwise silently
/// resolve under the default trust tier and lose the author's intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedImport {
    pub uri: String,
    #[serde(default)]
    pub kind: ImportKind,
}

/// Trust tier under which an [`ImportEntry`] resolves.
///
/// Per the preset infrastructure memo:
/// - [`ImportKind::Executable`] (default): the import is code (hooks,
///   lint plugins, runners). Signature-verified per spec §30, lockfile-
///   pinned, TOFU on first contact.
/// - [`ImportKind::Config`]: the import is inert TOML (presets). SHA-
///   pinned for reproducibility but NOT signature-verified. Applying
///   signature ceremony to a `severity = "warn"` TOML overlay is theatre
///   and would block bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportKind {
    Executable,
    Config,
}

impl Default for ImportKind {
    fn default() -> Self {
        Self::Executable
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtImport {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub runner: Option<String>,
}

/// `[lint-crates."<name>"]` entry. External lint pack reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LintCrateRef {
    pub git: String,
    pub rev: String,
}

/// `[lints.<name>]` entry. Per-lint gate severity plus open-ended extras.
///
/// The fixed fields (`commit`, `build`, `push`) cover gate severities at the
/// three pipeline points. `max_lines`, `forbidden`, `reason`, and any other
/// lint-specific extras are captured in `extras` for the lint implementation
/// to consume directly.
///
/// `extends` carries the optional preset shorthand (e.g.
/// `extends = "stack-lints::no-heap"` or `extends = "mockspace::no-bare-numeric"`).
/// The loader expands `<host>::<name>` into the full
/// `mock://(@|ext/<host>)/export/lint-preset/<name>` URI and resolves
/// the preset chain before applying this entry's overrides. Per the
/// preset infrastructure memo at
/// `mock/research/202605220500_lint-preset-infrastructure.md`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LintConfig {
    /// Preset shorthand: `<host>::<preset-name>`. See struct docs.
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub commit: Option<Severity>,
    #[serde(default)]
    pub build: Option<Severity>,
    #[serde(default)]
    pub push: Option<Severity>,
    /// `[lints.<name>.scope.<scope_name>]` entries for scoped configuration.
    #[serde(default)]
    pub scope: BTreeMap<String, ScopedLintConfig>,
    /// Remaining keys (e.g., `max_lines`, `forbidden`, `reason`).
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// Scoped override under `[lints.<name>.scope.<scope_name>]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopedLintConfig {
    #[serde(default)]
    pub commit: Option<Severity>,
    #[serde(default)]
    pub build: Option<Severity>,
    #[serde(default)]
    pub push: Option<Severity>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// Lint severity at one gate. Loader refuses unknown values.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
    Info,
    Off,
}

/// `[languages.<lang>]` entry. Either `"built-in"` or a git host pointer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LanguageEntry {
    /// `rust = "built-in"`
    BuiltIn(BuiltInLiteral),
    /// `typescript = { git = "...", rev = "..." }`
    Host(LanguageHost),
}

/// Newtype enforcing the literal string `"built-in"`.
#[derive(Debug, Clone, Copy, PartialEq, Hash, Serialize, Deserialize)]
pub enum BuiltInLiteral {
    #[serde(rename = "built-in")]
    BuiltIn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanguageHost {
    pub git: String,
    pub rev: String,
}

/// `[profile.<name>]` block. See spec §36 for the policy semantics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileSection {
    #[serde(default)]
    pub on_dirty_state: Option<OnDirtyState>,
    /// Remaining profile-specific keys. §36 enumerates the full list; this
    /// struct keeps them flat so additions don't require schema changes.
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnDirtyState {
    Prompt,
    Refuse,
    Auto,
}

/// `[crate_colors.<name>]` entry. Open-ended display metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CrateColor {
    pub fg: Option<String>,
    pub bg: Option<String>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// `[domain_kinds.<name>]` entry. Open-ended domain glyph + label.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DomainKind {
    pub glyph: Option<String>,
    pub label: Option<String>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// `[known_macros.<name>]` entry. Open-ended macro display metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KnownMacro {
    pub description: Option<String>,
    pub usage: Option<String>,
    #[serde(flatten)]
    pub extras: BTreeMap<String, toml::Value>,
}

/// `[transparency]` block (optional).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TransparencySection {
    #[serde(default)]
    pub log_uri: Option<String>,
    #[serde(default)]
    pub staleness_threshold_days: Option<u32>,
}

/// `[undo]` block. Undo log retention policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndoSection {
    #[serde(default = "default_undo_keep_entries")]
    pub keep_entries: u32,
    #[serde(default = "default_undo_keep_days")]
    pub keep_days: u32,
}

impl Default for UndoSection {
    fn default() -> Self {
        Self {
            keep_entries: default_undo_keep_entries(),
            keep_days: default_undo_keep_days(),
        }
    }
}

fn default_undo_keep_entries() -> u32 {
    50
}
fn default_undo_keep_days() -> u32 {
    30
}

// =========================================================================
// PresetFile: parsed `<name>.preset.toml` packaged under a lint preset
// export. Per the preset infrastructure memo at
// `mock/research/202605220500_lint-preset-infrastructure.md`.
// =========================================================================

/// A parsed lint preset TOML file.
///
/// Presets are inert TOML overlays packaged as exports under
/// `refs/mock/export/<package>/lint-preset/<name>`. They name a
/// catalog primitive and supply a configuration overlay (forbidden
/// patterns, severities, scope filters, reason text) without
/// shipping any executable code.
///
/// The cascade (catalog defaults → preset chain → workspace defaults
/// → per-lint TOML → CLI overrides) applies preset state below
/// workspace defaults. Consumers retain full override authority; a
/// preset cannot dictate behaviour the consumer's TOML has pinned.
///
/// `deny_unknown_fields` catches typos in preset files (e.g.
/// `extens = "..."` instead of `extends`) at load time rather than
/// silently dropping the field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetFile {
    /// Preset schema version. Loader rejects unknown major versions.
    pub schema_version: String,
    /// Preset name; combined with the host name to form the
    /// `<host>::<name>` shorthand at consumer sites.
    pub name: String,
    /// Catalog primitive this preset configures (e.g. `forbidden_imports`,
    /// `token_scan`). Must match a registered catalog kind.
    pub primitive: String,
    /// Human-readable summary surfaced by `cargo mock explain`.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional shorthand to another preset whose overrides apply
    /// before this preset's. Resolution walks the chain innermost-
    /// extends-target first; cycles are a hard load-time error.
    #[serde(default)]
    pub extends: Option<String>,
    /// Configuration overlay applied over the primitive's catalog
    /// defaults. Keys depend on the primitive (e.g.
    /// `forbidden`, `reason`, `max_lines`). The loader passes the
    /// merged table to the primitive's `instantiate` constructor.
    #[serde(default)]
    pub config: BTreeMap<String, toml::Value>,
    /// Per-gate default severities for the preset. Scalar overrides
    /// from `[lints.<name>]` per-lint TOML win.
    #[serde(default)]
    pub severity: GateSeverities,
    /// Scope overlay applied over the primitive's default scope.
    /// Same axes as [`ScopedLintConfig`]; list-merge semantics with
    /// `.add` / `.remove` sub-keys resolve at the cascade layer.
    #[serde(default)]
    pub scope: BTreeMap<String, toml::Value>,
}

/// Per-gate severity overrides at the three pipeline points.
///
/// Used by [`PresetFile::severity`] today. `LintConfig` and
/// [`ScopedLintConfig`] carry the same three fields inline; a
/// follow-up (#538) migrates those sites to this shared type so the
/// wire shape stays identical and the Rust types deduplicate.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSeverities {
    #[serde(default)]
    pub commit: Option<Severity>,
    #[serde(default)]
    pub build: Option<Severity>,
    #[serde(default)]
    pub push: Option<Severity>,
}
