//! Configuration for a mockspace workspace.
//!
//! Reads `mockspace.toml` from the mockspace root directory.

use std::fs;
use std::path::{Path, PathBuf};

/// How mockspace-managed content is installed into existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Overwrite the entire file. Default for fully-generated files.
    Replace,
    /// Append a managed section after existing content. Existing content
    /// is preserved. The managed section is identified by begin/end markers
    /// and updated in-place on subsequent runs.
    MergeAppend,
    /// Prepend a managed section before existing content.
    MergePrepend,
    /// Don't install if the file already exists.
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
    /// Root directory of the mock workspace (where mockspace.toml lives).
    pub mock_dir: PathBuf,
    /// Directory containing mock crates.
    pub crates_dir: PathBuf,
    /// Root directory of the consuming project (repo root).
    pub repo_root: PathBuf,
    /// Directory where generated docs are written.
    pub docs_dir: PathBuf,
    /// Project name (used in generated docs and graphs).
    pub project_name: String,
    /// Crate name prefix (e.g. "loimu" → crates are "loimu-signal", etc.).
    pub crate_prefix: String,
    /// Proc-macro crate names (exempt from collection/box/float lints).
    pub proc_macro_crates: Vec<String>,
    /// Module crates that produce dylib outputs (for ABI verification).
    pub module_crates: Vec<String>,
    /// Expected dylib ABI version.
    pub abi_version: u32,
    /// Nuke marker text used in lib.rs stubs.
    pub nuke_marker: String,
    /// Commit message style.
    pub commit_style: CommitStyle,
    /// Install mode for git hooks.
    pub install_git_hooks: InstallMode,
    /// Install mode for cargo config (alias).
    pub install_cargo_config: InstallMode,
    /// Install mode for agent files (CLAUDE.md, rules, skills, etc.).
    pub install_agent_files: InstallMode,
}

/// Commit message format configuration.
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
    /// Load configuration from a mockspace.toml file, or use defaults
    /// derived from directory layout.
    ///
    /// The `mock_dir` is the root of the mock workspace (the directory
    /// containing the `crates/` subdirectory and doc templates).
    pub fn from_dir(mock_dir: &Path) -> Self {
        let mock_dir = mock_dir.to_path_buf();
        let crates_dir = mock_dir.join("crates");

        // Walk up to find the repo root (directory containing .git)
        let repo_root = find_repo_root(&mock_dir)
            .unwrap_or_else(|| mock_dir.clone());

        // Docs dir is relative to repo root
        let docs_dir = repo_root.join("docs");

        // Try to read mockspace.toml for overrides
        let toml_path = mock_dir.join("mockspace.toml");
        let toml_content = fs::read_to_string(&toml_path).unwrap_or_default();

        let project_name = parse_toml_string(&toml_content, "project_name")
            .unwrap_or_else(|| "project".into());
        let crate_prefix = parse_toml_string(&toml_content, "crate_prefix")
            .unwrap_or_else(|| project_name.clone());

        let proc_macro_crates = parse_toml_string_array(&toml_content, "proc_macro_crates");
        let module_crates = parse_toml_string_array(&toml_content, "module_crates");

        let abi_version = parse_toml_u32(&toml_content, "abi_version").unwrap_or(1);

        let nuke_marker = format!("Nuked by `cargo mock --nuke`");

        let install_git_hooks = parse_toml_string(&toml_content, "install_git_hooks")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);

        let install_cargo_config = parse_toml_string(&toml_content, "install_cargo_config")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::MergeAppend);

        let install_agent_files = parse_toml_string(&toml_content, "install_agent_files")
            .and_then(|s| InstallMode::parse(&s))
            .unwrap_or(InstallMode::Replace);

        Config {
            mock_dir,
            crates_dir,
            repo_root,
            docs_dir,
            project_name,
            crate_prefix,
            proc_macro_crates,
            module_crates,
            abi_version,
            nuke_marker,
            commit_style: CommitStyle::default(),
            install_git_hooks,
            install_cargo_config,
            install_agent_files,
        }
    }
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

// Minimal TOML parsing (no external dependency).

fn parse_toml_string(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) {
            if let Some(after_eq) = trimmed.split_once('=') {
                let val = after_eq.1.trim().trim_matches('"');
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_toml_string_array(content: &str, key: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_array = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) && trimmed.contains('[') {
            in_array = true;
            // Inline array
            if let Some(start) = trimmed.find('[') {
                if let Some(end) = trimmed.find(']') {
                    let inner = &trimmed[start + 1..end];
                    for item in inner.split(',') {
                        let val = item.trim().trim_matches('"').trim_matches('\'');
                        if !val.is_empty() {
                            result.push(val.to_string());
                        }
                    }
                    return result;
                }
            }
            continue;
        }
        if in_array {
            if trimmed == "]" {
                break;
            }
            let val = trimmed.trim_matches(',').trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                result.push(val.to_string());
            }
        }
    }
    result
}

fn parse_toml_u32(content: &str, key: &str) -> Option<u32> {
    parse_toml_string(content, key)?.parse().ok()
}
