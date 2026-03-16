//! Configuration for a mockspace workspace.
//!
//! Reads `mockspace.toml` from the mockspace root directory.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::IntoDeserializer;

use mockspace_lint_rules::{Level, Severity, parse_severity, LintConfig};

/// How mockspace-managed content is installed into existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub struct Config {
    // --- Core fields ---
    pub mock_dir: PathBuf,
    pub crates_dir: PathBuf,
    pub repo_root: PathBuf,
    pub docs_dir: PathBuf,
    pub project_name: String,
    pub crate_prefix: String,
    pub proc_macro_crates: Vec<String>,
    pub module_crates: Vec<String>,
    pub abi_version: u32,
    pub nuke_marker: String,
    pub commit_style: CommitStyle,
    pub install_git_hooks: InstallMode,
    pub install_cargo_config: InstallMode,
    pub install_agent_files: InstallMode,

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
}

#[derive(Clone)]
pub struct MacroStyle {
    pub label: String,
    pub icon: String,
    pub bg: String,
    pub fg: String,
}

impl MacroStyle {
    pub fn default_for(macro_name: &str) -> Self {
        Self {
            label: macro_name.strip_prefix("define_").unwrap_or("generated").to_string(),
            icon: "\u{2699}".to_string(),
            bg: "#F5F5F5".to_string(),
            fg: "#616161".to_string(),
        }
    }
}

pub struct CommitStyle {
    pub types: Vec<String>,
    pub format: String,
}

impl Default for CommitStyle {
    fn default() -> Self {
        Self {
            types: vec![
                "feat".into(), "fix".into(), "refactor".into(),
                "docs".into(), "test".into(), "chore".into(),
            ],
            format: "type: lowercase imperative message".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Serde deserialization structs
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    project_name: Option<String>,
    crate_prefix: Option<String>,
    abi_version: Option<u32>,
    proc_macro_crates: Vec<String>,
    module_crates: Vec<String>,
    layers: Vec<String>,
    primary_domain_macro: Option<String>,
    primary_domain_label: Option<String>,
    install_git_hooks: Option<String>,
    install_cargo_config: Option<String>,
    install_agent_files: Option<String>,

    // Sections (simple key=value maps)
    domain_kinds: Option<BTreeMap<String, String>>,
    known_macros: Option<BTreeMap<String, String>>,
    agent_macros: Option<BTreeMap<String, String>>,
    macro_styles: Option<BTreeMap<String, String>>,
    crate_colors: Option<BTreeMap<String, String>>,
    crate_grouping: Option<BTreeMap<String, String>>,

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
    commit: Option<String>,
    build: Option<String>,
    push: Option<String>,
    severity: Option<String>,
    findings: Option<BTreeMap<String, String>>,
    rule: Option<BTreeMap<String, BTreeMap<String, StringOrOther>>>,
    /// Inline array of tables format:
    /// `rules = [{ scope = "...", forbidden = "...", reason = "..." }]`
    rules: Option<Vec<InlineRule>>,
    #[serde(flatten)]
    params: BTreeMap<String, StringOrOther>,
}

/// A single forbidden-imports rule in inline array format.
#[derive(Deserialize)]
struct InlineRule {
    scope: String,
    forbidden: String,
    reason: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool { true }

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
    pub fn from_dir(mock_dir: &Path) -> Self {
        let mock_dir = mock_dir.to_path_buf();
        let crates_dir = mock_dir.join("crates");

        let repo_root = find_repo_root(&mock_dir)
            .unwrap_or_else(|| mock_dir.clone());
        let docs_dir = repo_root.join("docs");

        let toml_path = mock_dir.join("mockspace.toml");
        let toml_content = fs::read_to_string(&toml_path).unwrap_or_default();

        let raw: RawConfig = toml_edit::de::from_str(&toml_content)
            .unwrap_or_default();

        let project_name = raw.project_name
            .unwrap_or_else(|| "project".into());
        let crate_prefix = raw.crate_prefix
            .unwrap_or_else(|| project_name.clone());

        let install_git_hooks = raw.install_git_hooks
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);
        let install_cargo_config = raw.install_cargo_config
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::MergeAppend);
        let install_agent_files = raw.install_agent_files
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);

        let domain_kinds = raw.domain_kinds.unwrap_or_default();

        let known_macros = pipe2_section(raw.known_macros);
        let agent_macros = pipe2_section(raw.agent_macros);

        let macro_styles = convert_macro_styles(raw.macro_styles, &domain_kinds);
        let crate_colors = convert_color_section(raw.crate_colors);

        let lint_overrides = parse_lints_from_document(&toml_content, &crate_prefix);

        let primary_domain_label = raw.primary_domain_label
            .unwrap_or_else(|| "Items".to_string());

        Config {
            mock_dir, crates_dir, repo_root, docs_dir,
            project_name, crate_prefix,
            proc_macro_crates: raw.proc_macro_crates,
            module_crates: raw.module_crates,
            abi_version: raw.abi_version.unwrap_or(1),
            nuke_marker: "Nuked by `cargo mock --nuke`".to_string(),
            commit_style: CommitStyle::default(),
            install_git_hooks, install_cargo_config, install_agent_files,
            lint_overrides,
            domain_kinds,
            known_macros, agent_macros,
            macro_styles, crate_colors,
            layer_labels: raw.layers,
            primary_domain_macro: raw.primary_domain_macro,
            primary_domain_label,
            crate_grouping: raw.crate_grouping.unwrap_or_default(),
        }
    }

    /// Get domain kind label for a macro name (e.g. "define_signal" -> "signal").
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
    pub fn macro_style(&self, macro_name: &str) -> MacroStyle {
        self.macro_styles.get(macro_name)
            .cloned()
            .unwrap_or_else(|| MacroStyle::default_for(macro_name))
    }

    /// Get crate header colors for graph. Returns (bg, fg).
    pub fn crate_color(&self, short_name: &str) -> (String, String) {
        self.crate_colors.get(short_name)
            .cloned()
            .unwrap_or_else(|| ("#F0F0F0".to_string(), "#666666".to_string()))
    }

    /// Get layer label for a depth index.
    pub fn layer_label(&self, depth: usize) -> String {
        self.layer_labels.get(depth)
            .cloned()
            .unwrap_or_else(|| "Other".to_string())
    }

    /// Get the effective known_macros for agent templates.
    /// Returns agent_macros if non-empty, otherwise known_macros.
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
    raw.into_iter().map(|(key, val)| {
        let parts: Vec<&str> = val.splitn(2, '|').map(|s| s.trim()).collect();
        match parts.len() {
            1 => (key, parts[0].to_string(), String::new()),
            _ => (key, parts[0].to_string(), parts[1].to_string()),
        }
    }).collect()
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
                icon: parts[1].to_string(),
                bg: parts[2].to_string(),
                fg: parts[3].to_string(),
            });
        } else if parts.len() == 3 {
            // 3-field: icon | bg | fg (label from domain_kinds)
            let label = domain_kinds.get(&name)
                .map(|dk| dk.chars().skip_while(|c| !c.is_ascii_alphanumeric())
                    .collect::<String>().trim().to_string())
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
            }
            LintEntry::Config(table) => {
                // Determine base severity from "severity" key
                if let Some(ref s) = table.severity {
                    if let Some(severity) = parse_severity(s) {
                        base.insert(lint_name.clone(), severity);
                    }
                }

                // Per-gate severity from commit/build/push keys
                let has_gates = table.commit.is_some()
                    || table.build.is_some()
                    || table.push.is_some();
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

                // Named rule sub-tables: [lints.lint-name.rule.rule-name]
                if let Some(rule_map) = table.rule {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (rule_name, rule_fields) in rule_map {
                        for (key, val) in rule_fields {
                            let val_str = val.into_string()
                                .replace("{prefix}", crate_prefix);
                            let param_key = format!("rule.{rule_name}.{key}");
                            param_entry.insert(param_key, val_str);
                        }
                    }
                }

                // Inline rules array: rules = [{ scope, forbidden, reason }]
                if let Some(rules_array) = table.rules {
                    let param_entry = params.entry(lint_name.clone()).or_default();
                    for (idx, rule) in rules_array.into_iter().enumerate() {
                        if !rule.enabled { continue; }
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
                        let val_str = val.into_string()
                            .replace("{prefix}", crate_prefix);
                        param_entry.insert(key, val_str);
                    }
                }
            }
        }
    }

    LintConfig { base, findings, params }
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
