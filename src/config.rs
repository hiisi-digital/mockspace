//! Configuration for a mockspace workspace.
//!
//! Reads `mockspace.toml` from the mockspace root directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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

    // --- Domain-specific config ---

    /// Macro icon+label for STRUCTURE.md domain items.
    /// e.g. "define_signal" → "📡 signal"
    pub domain_kinds: BTreeMap<String, String>,

    /// Known macros for DESIGN.md table. (name, description, usage)
    pub known_macros: Vec<(String, String, String)>,

    /// Known macros for agent instructions table. (name, purpose, usage)
    /// If empty, falls back to known_macros.
    pub agent_macros: Vec<(String, String, String)>,

    /// Macro graph styling: name → (label, icon, bg_color, fg_color)
    pub macro_styles: BTreeMap<String, MacroStyle>,

    /// Crate header colors for graph: short_name → (bg, fg)
    pub crate_colors: BTreeMap<String, (String, String)>,

    /// Layer labels by depth index.
    pub layer_labels: Vec<String>,

    /// Which macro type to track per-crate for {{signals_per_crate}}.
    pub primary_domain_macro: Option<String>,

    /// Label for the primary domain macro column (e.g. "Signals").
    pub primary_domain_label: String,

    /// Crate companion grouping for graph rank: source → target.
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
            icon: "⚙".to_string(),
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

impl Config {
    pub fn from_dir(mock_dir: &Path) -> Self {
        let mock_dir = mock_dir.to_path_buf();
        let crates_dir = mock_dir.join("crates");

        let repo_root = find_repo_root(&mock_dir)
            .unwrap_or_else(|| mock_dir.clone());
        let docs_dir = repo_root.join("docs");

        let toml_path = mock_dir.join("mockspace.toml");
        let toml_content = fs::read_to_string(&toml_path).unwrap_or_default();

        let project_name = parse_string(&toml_content, "project_name")
            .unwrap_or_else(|| "project".into());
        let crate_prefix = parse_string(&toml_content, "crate_prefix")
            .unwrap_or_else(|| project_name.clone());
        let proc_macro_crates = parse_string_array(&toml_content, "proc_macro_crates");
        let module_crates = parse_string_array(&toml_content, "module_crates");
        let abi_version = parse_u32(&toml_content, "abi_version").unwrap_or(1);
        let nuke_marker = "Nuked by `cargo mock --nuke`".to_string();

        let install_git_hooks = parse_string(&toml_content, "install_git_hooks")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);
        let install_cargo_config = parse_string(&toml_content, "install_cargo_config")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::MergeAppend);
        let install_agent_files = parse_string(&toml_content, "install_agent_files")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);

        // --- Domain-specific config ---

        let domain_kinds = parse_section(&toml_content, "domain_kinds");

        let known_macros = parse_section_pipe2(&toml_content, "known_macros");
        let agent_macros = parse_section_pipe2(&toml_content, "agent_macros");

        let macro_styles = parse_macro_styles(&toml_content, &domain_kinds);
        let crate_colors = parse_color_section(&toml_content, "crate_colors");

        let layer_labels = parse_string_array(&toml_content, "layers");

        let primary_domain_macro = parse_string(&toml_content, "primary_domain_macro");
        let primary_domain_label = parse_string(&toml_content, "primary_domain_label")
            .unwrap_or_else(|| "Items".to_string());

        let crate_grouping = parse_section(&toml_content, "crate_grouping");

        Config {
            mock_dir, crates_dir, repo_root, docs_dir,
            project_name, crate_prefix,
            proc_macro_crates, module_crates, abi_version, nuke_marker,
            commit_style: CommitStyle::default(),
            install_git_hooks, install_cargo_config, install_agent_files,
            domain_kinds, known_macros, agent_macros,
            macro_styles, crate_colors, layer_labels,
            primary_domain_macro, primary_domain_label,
            crate_grouping,
        }
    }

    /// Get domain kind label for a macro name (e.g. "define_signal" → "📡 signal").
    pub fn domain_kind(&self, macro_name: &str) -> String {
        if let Some(label) = self.domain_kinds.get(macro_name) {
            return label.clone();
        }
        match macro_name.strip_prefix("define_") {
            Some(kind) => format!("⚙ {kind}"),
            None => "⚙ generated".to_string(),
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
            .unwrap_or_else(|| format!("Layer {depth}"))
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
// TOML parsing helpers (minimal, no external dependency)
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

/// Parse a top-level `key = "value"` (not inside any [section]).
fn parse_string(content: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            in_section = true;
            continue;
        }
        if in_section { continue; }
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Parse a top-level `key = [array]`.
fn parse_string_array(content: &str, key: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_array = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if !in_array && trimmed.starts_with(key) && trimmed.contains('[') {
            in_array = true;
            // Inline array?
            if let (Some(s), Some(e)) = (trimmed.find('['), trimmed.find(']')) {
                for item in trimmed[s + 1..e].split(',') {
                    let val = item.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() { result.push(val.to_string()); }
                }
                return result;
            }
            continue;
        }
        if in_array {
            if trimmed == "]" { break; }
            let val = trimmed.trim_matches(',').trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() { result.push(val.to_string()); }
        }
    }
    result
}

fn parse_u32(content: &str, key: &str) -> Option<u32> {
    parse_string(content, key)?.parse().ok()
}

/// Parse all key=value pairs inside a `[section_name]` block.
fn parse_section(content: &str, section_name: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let mut in_section = false;
    let header = format!("[{section_name}]");

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == header {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('[') { break; }
        if in_section && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((k, v)) = trimmed.split_once('=') {
                result.insert(
                    k.trim().to_string(),
                    v.trim().trim_matches('"').to_string(),
                );
            }
        }
    }
    result
}

/// Parse a section with pipe-separated 2-field values.
/// Format: `key = "field1 | field2"`
/// Returns Vec<(key, field1, field2)> preserving insertion order.
fn parse_section_pipe2(content: &str, section_name: &str) -> Vec<(String, String, String)> {
    let raw = parse_section(content, section_name);
    // Use BTreeMap ordering (alphabetical by key)
    raw.into_iter().map(|(key, val)| {
        let parts: Vec<&str> = val.splitn(2, '|').map(|s| s.trim()).collect();
        match parts.len() {
            1 => (key, parts[0].to_string(), String::new()),
            _ => (key, parts[0].to_string(), parts[1].to_string()),
        }
    }).collect()
}

/// Parse `[macro_styles]` section.
/// Format: `define_foo = "label | icon | bg_color | fg_color"`
fn parse_macro_styles(content: &str, domain_kinds: &BTreeMap<String, String>) -> BTreeMap<String, MacroStyle> {
    let raw = parse_section(content, "macro_styles");
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

/// Parse a section with pipe-separated color pairs: `key = "bg | fg"`
fn parse_color_section(content: &str, section_name: &str) -> BTreeMap<String, (String, String)> {
    let raw = parse_section(content, section_name);
    let mut result = BTreeMap::new();
    for (name, val) in raw {
        let parts: Vec<&str> = val.splitn(2, '|').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            result.insert(name, (parts[0].to_string(), parts[1].to_string()));
        }
    }
    result
}
