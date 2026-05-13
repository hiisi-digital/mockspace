//! Canonical mockspace.toml schema types.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The canonical typed configuration mockspace templates render against.
///
/// Every mockspace template references fields on this struct by name
/// (`{{ project_name }}`, `{{ mock_dir }}`, etc.). Consumer tools whose
/// configs do not natively match this shape implement `IntoMockspaceConfig`
/// to map their schema onto these field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_mock_dir")]
    pub mock_dir: PathBuf,
    #[serde(default = "default_crates_dir")]
    pub crates_dir: PathBuf,
    #[serde(default = "default_repo_root")]
    pub repo_root: PathBuf,
    #[serde(default = "default_docs_dir")]
    pub docs_dir: PathBuf,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub crate_prefix: String,
    #[serde(default)]
    pub proc_macro_crates: Vec<String>,
    #[serde(default)]
    pub lint_proc_macro_source: bool,
    #[serde(default)]
    pub module_crates: Vec<String>,
    #[serde(default)]
    pub unprefixed_crates: Vec<String>,
    #[serde(default = "default_abi_version")]
    pub abi_version: u32,
    #[serde(default = "default_nuke_marker")]
    pub nuke_marker: String,
    #[serde(default)]
    pub commit_style: CommitStyle,
    #[serde(default)]
    pub install_git_hooks: InstallMode,
    #[serde(default)]
    pub install_cargo_config: InstallMode,
    #[serde(default)]
    pub install_agent_files: InstallMode,
    #[serde(default)]
    pub attribution: AttributionConfig,
    #[serde(default)]
    pub lint_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub domain_kinds: BTreeMap<String, String>,
}

fn default_mock_dir() -> PathBuf {
    PathBuf::from("mock")
}
fn default_crates_dir() -> PathBuf {
    PathBuf::from("mock/crates")
}
fn default_repo_root() -> PathBuf {
    PathBuf::from(".")
}
fn default_docs_dir() -> PathBuf {
    PathBuf::from("docs")
}
fn default_abi_version() -> u32 {
    1
}
fn default_nuke_marker() -> String {
    "MOCKSPACE_NUKE".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mock_dir: default_mock_dir(),
            crates_dir: default_crates_dir(),
            repo_root: default_repo_root(),
            docs_dir: default_docs_dir(),
            project_name: String::new(),
            crate_prefix: String::new(),
            proc_macro_crates: Vec::new(),
            lint_proc_macro_source: false,
            module_crates: Vec::new(),
            unprefixed_crates: Vec::new(),
            abi_version: default_abi_version(),
            nuke_marker: default_nuke_marker(),
            commit_style: CommitStyle::default(),
            install_git_hooks: InstallMode::default(),
            install_cargo_config: InstallMode::default(),
            install_agent_files: InstallMode::default(),
            attribution: AttributionConfig::default(),
            lint_overrides: BTreeMap::new(),
            domain_kinds: BTreeMap::new(),
        }
    }
}

/// How mockspace-managed content is installed into existing files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMode {
    Replace,
    MergeAppend,
    MergePrepend,
    Skip,
}

impl Default for InstallMode {
    fn default() -> Self {
        Self::Replace
    }
}

/// Agent attribution policy (byline rules, forbidden suffixes, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttributionConfig {
    #[serde(default)]
    pub agent_byline_required: bool,
    #[serde(default)]
    pub forbidden_suffixes: Vec<String>,
}

/// Commit message conventions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommitStyle {
    #[serde(default)]
    pub max_subject_len: Option<usize>,
    #[serde(default)]
    pub allowed_types: Vec<String>,
}

/// Domain-specific macro labeling (icon + label per macro kind).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MacroStyle {
    pub icon: Option<String>,
    pub label: Option<String>,
}
