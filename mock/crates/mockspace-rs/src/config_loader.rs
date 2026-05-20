//! TOML configuration loader and override cascade.
//!
//! Per schema design memo §11. Cascade precedence, lowest-to-highest:
//!
//! 1. `CatalogEntry::default_config` + `default_scope` + `default_severity`.
//! 2. Workspace-level `[lints]` defaults.
//! 3. Per-lint `[lints.<name>]` blocks in `lints.toml`.
//! 4. CLI overrides (`--scope`, `--lint`, `--severity-override`).
//!
//! The two-channel return shape (`(entries, config_errors)`) keeps source
//! findings and configuration faults separate per §9.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::catalog::CatalogEntry;
use crate::errors::{ConfigError, ConfigErrorKind, LoadError};
use crate::lint::{Lint, LintMode};

// =========================================================================
// Cascade input.
// =========================================================================

/// CLI-level overrides applied last in the cascade.
#[derive(Debug, Clone, Default)]
pub struct OverrideCascade {
    /// `--scope <crate>`: intersected with each lint's `scope.crates`.
    /// Empty disables the intersection.
    pub scope_intersection: Vec<String>,
    /// `--lint <name>`: limits the active set to the named lint(s).
    /// `None` means "no filter; run every loaded lint".
    pub lint_filter: Option<Vec<String>>,
    /// `--severity-override <name>=<sev>`: bumps the named lint's
    /// effective severity (engine applies at the active gate).
    pub severity_overrides: HashMap<String, mockspace_core::lint::Severity>,
}

// =========================================================================
// TOML schema (deserialise target).
// =========================================================================

/// Top-level structure of `lints.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LintsTomlFile {
    #[serde(default)]
    pub lints: HashMap<String, toml::Table>,
    /// Workspace-level defaults that apply to every lint unless its own
    /// block overrides them.
    #[serde(default)]
    pub defaults: Option<toml::Table>,
}

// =========================================================================
// Output.
// =========================================================================

/// Per-gate `only_staged` flag triple. Parsed from the per-lint TOML
/// `[lints.<name>.gate.<g>].only_staged` blocks; consulted by the engine
/// at dispatch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OnlyStaged {
    pub commit: bool,
    pub build: bool,
    pub push: bool,
}

impl OnlyStaged {
    pub fn at(self, gate: mockspace_core::lint::Gate) -> bool {
        match gate {
            mockspace_core::lint::Gate::Commit => self.commit,
            mockspace_core::lint::Gate::Build => self.build,
            mockspace_core::lint::Gate::Push => self.push,
        }
    }
}

/// One instantiated lint plus the catalog metadata the engine needs at
/// dispatch time.
pub struct InstantiatedLint {
    pub lint: Box<dyn Lint>,
    pub mode: LintMode,
    pub staging_aware: bool,
    pub editor_skip: bool,
    pub only_staged: OnlyStaged,
    /// Pre-compiled scope filter (path globs, crate patterns, language
    /// whitelist, proc-macro exempt, category exempt). Consulted per
    /// document at dispatch.
    pub scope_filter: crate::scope_filter::ScopeFilter,
}

/// Parsed configuration plus instantiated lints, ready for engine
/// dispatch. The split between `entries` and `config_errors` is the
/// two-channel return shape.
pub struct LintsConfig {
    pub entries: Vec<InstantiatedLint>,
    pub config_errors: Vec<ConfigError>,
}

impl std::fmt::Debug for LintsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LintsConfig")
            .field("entries", &self.entries.len())
            .field("config_errors", &self.config_errors.len())
            .finish()
    }
}

impl LintsConfig {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            config_errors: Vec::new(),
        }
    }

    /// Catalog-only load: every registered entry instantiates with its
    /// own static defaults. No user TOML, no CLI overrides.
    pub fn from_catalog_defaults() -> Self {
        Self::from_inputs(LintsTomlFile::default(), OverrideCascade::default())
    }

    /// Full cascade load. Reads `lints.toml` from one of the conventional
    /// locations relative to `workspace_root`:
    ///
    /// - `<root>/lints.toml`
    /// - `<root>/mock/lints.toml`
    /// - `<root>/mockspace.toml` (legacy; only the `[lints]` table is read)
    ///
    /// The first that exists wins. Absence is not an error; the cascade
    /// degrades to catalog defaults.
    pub fn load(
        workspace_root: &Path,
        overrides: OverrideCascade,
    ) -> Result<Self, LoadError> {
        let user_toml = find_and_read_lints_toml(workspace_root)?;
        Ok(Self::from_inputs(user_toml, overrides))
    }

    /// Direct cascade entry point: pass parsed user TOML + overrides.
    /// Test-friendly form; production callers use [`Self::load`].
    pub fn from_inputs(
        user_toml: LintsTomlFile,
        overrides: OverrideCascade,
    ) -> Self {
        let mut entries = Vec::new();
        let mut config_errors = Vec::new();

        // Workspace-default block (low-precedence overlay over each lint
        // sub-table, applied at the engine merge step below).
        let workspace_defaults = user_toml.defaults.clone().unwrap_or_default();

        for entry in crate::catalog::catalog_entries() {
            // Lint filter: skip lints not in the CLI --lint set.
            if let Some(filter) = &overrides.lint_filter {
                if !filter.iter().any(|n| n == entry.name) {
                    continue;
                }
            }

            match instantiate_with_cascade(
                entry,
                &user_toml.lints,
                &workspace_defaults,
                &overrides,
            ) {
                Ok(lint) => entries.push(lint),
                Err(e) => config_errors.push(e),
            }
        }
        Self {
            entries,
            config_errors,
        }
    }
}

// =========================================================================
// Per-entry cascade execution.
// =========================================================================

fn extract_only_staged(
    entry: &CatalogEntry,
    user_block: Option<&toml::Table>,
) -> Result<OnlyStaged, ConfigError> {
    let mut out = OnlyStaged::default();
    let Some(block) = user_block else {
        return Ok(out);
    };
    let Some(toml::Value::Table(gate_root)) = block.get("gate") else {
        return Ok(out);
    };
    for (gate_name, gate_value) in gate_root {
        let toml::Value::Table(g) = gate_value else {
            continue;
        };
        let Some(toml::Value::Boolean(only_staged)) = g.get("only_staged") else {
            continue;
        };
        if *only_staged && !entry.staging_aware {
            return Err(ConfigError {
                lint_name: entry.name.to_string(),
                field_path: format!("gate.{gate_name}.only_staged"),
                kind: crate::errors::ConfigErrorKind::ContradictsCatalog,
                message: format!(
                    "lint `{}` is not staging_aware; `only_staged = true` is invalid here",
                    entry.name
                ),
                source_location: None,
            });
        }
        match gate_name.as_str() {
            "commit" => out.commit = *only_staged,
            "build" => out.build = *only_staged,
            "push" => out.push = *only_staged,
            other => {
                return Err(ConfigError {
                    lint_name: entry.name.to_string(),
                    field_path: format!("gate.{other}"),
                    kind: crate::errors::ConfigErrorKind::UnknownField,
                    message: format!("unknown gate `{other}` (expected commit / build / push)"),
                    source_location: None,
                });
            }
        }
    }
    Ok(out)
}

fn instantiate_with_cascade(
    entry: &CatalogEntry,
    user_lints: &HashMap<String, toml::Table>,
    workspace_defaults: &toml::Table,
    overrides: &OverrideCascade,
) -> Result<InstantiatedLint, ConfigError> {
    // Level 1: catalog defaults.
    let mut merged_config: toml::Table =
        entry
            .default_config
            .parse()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: entry.name.to_string(),
                field_path: "default_config".to_string(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("default_config did not parse: {e}"),
                source_location: None,
            })?;
    let mut merged_scope: toml::Table =
        entry
            .default_scope
            .parse()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: entry.name.to_string(),
                field_path: "default_scope".to_string(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("default_scope did not parse: {e}"),
                source_location: None,
            })?;

    // Level 2: workspace defaults (overlay where present).
    overlay(&mut merged_config, workspace_defaults);

    // Level 3: per-lint user TOML.
    let user_block = user_lints.get(entry.name);
    if let Some(block) = user_block {
        if let Some(toml::Value::Table(t)) = block.get("config") {
            overlay(&mut merged_config, t);
        }
        if let Some(toml::Value::Table(t)) = block.get("scope") {
            overlay(&mut merged_scope, t);
        }
        // Per-finding-kind severity overrides validate against the
        // catalog's declared finding_kinds set.
        if let Some(toml::Value::Table(gate_root)) = block.get("gate") {
            for (gate_name, gate_value) in gate_root {
                if let toml::Value::Table(gate_block) = gate_value {
                    if let Some(toml::Value::Table(fk)) = gate_block.get("finding_kinds") {
                        let declared: HashSet<&str> = entry.finding_kinds.iter().copied().collect();
                        for (kind_key, _) in fk {
                            if !entry.finding_kinds.is_empty()
                                && !declared.contains(kind_key.as_str())
                            {
                                return Err(ConfigError {
                                    lint_name: entry.name.to_string(),
                                    field_path: format!(
                                        "gate.{gate_name}.finding_kinds.{kind_key}"
                                    ),
                                    kind: ConfigErrorKind::UnknownFindingKind,
                                    message: format!(
                                        "finding kind `{kind_key}` is not declared in catalog entry"
                                    ),
                                    source_location: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // only_staged extraction. staging_aware-consistency + unknown-gate
    // validation lives in extract_only_staged.
    let only_staged = extract_only_staged(entry, user_block)?;

    // Level 4: CLI scope intersection. The catalog entry's `scope.crates`
    // (or merged_scope.crates) is intersected with `overrides.scope_intersection`.
    if !overrides.scope_intersection.is_empty() {
        let existing = merged_scope
            .get("crates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let existing_set: HashSet<String> = existing
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        let cli_set: HashSet<String> =
            overrides.scope_intersection.iter().cloned().collect();
        let intersected: Vec<toml::Value> = if existing_set.is_empty()
            || existing.iter().any(|v| v.as_str() == Some("*"))
        {
            cli_set.iter().cloned().map(toml::Value::String).collect()
        } else {
            existing_set
                .intersection(&cli_set)
                .cloned()
                .map(toml::Value::String)
                .collect()
        };
        merged_scope.insert("crates".to_string(), toml::Value::Array(intersected));
    }

    // Parse merged_scope as ScopeConfig and compile the per-document
    // filter. The ScopeConfig::deserialize path validates field shapes;
    // ScopeFilter::from_config compiles glob sets and surfaces glob-parse
    // errors as ConfigError.
    let scope_config: crate::config_types::ScopeConfig =
        toml::Value::Table(merged_scope.clone())
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: entry.name.to_string(),
                field_path: "scope".to_string(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("scope config parse failed: {e}"),
                source_location: None,
            })?;
    let scope_filter =
        crate::scope_filter::ScopeFilter::from_config(entry.name, &scope_config)?;

    // Construct the lint.
    let lint = (entry.instantiate)(&merged_config, &merged_scope)?;
    Ok(InstantiatedLint {
        lint,
        mode: entry.mode,
        staging_aware: entry.staging_aware,
        editor_skip: entry.editor_skip,
        only_staged,
        scope_filter,
    })
}

// =========================================================================
// TOML merge.
// =========================================================================

/// Shallow overlay: keys in `src` overwrite keys in `dst` at the top
/// level. Nested tables get recursive overlay.
fn overlay(dst: &mut toml::Table, src: &toml::Table) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(toml::Value::Table(d)), toml::Value::Table(s)) => overlay(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

// =========================================================================
// File discovery.
// =========================================================================

fn find_and_read_lints_toml(workspace_root: &Path) -> Result<LintsTomlFile, LoadError> {
    let candidates = [
        workspace_root.join("lints.toml"),
        workspace_root.join("mock").join("lints.toml"),
        workspace_root.join("mockspace.toml"),
    ];
    for path in &candidates {
        if path.exists() {
            let contents = std::fs::read_to_string(path).map_err(|e| LoadError::Io {
                context: format!("reading {}", path.display()),
                source: e,
            })?;
            let parsed: LintsTomlFile = toml::from_str(&contents).map_err(|e| {
                LoadError::Config(vec![ConfigError {
                    lint_name: String::new(),
                    field_path: path.display().to_string(),
                    kind: ConfigErrorKind::InvalidValue,
                    message: format!("parse {}: {e}", path.display()),
                    source_location: None,
                }])
            })?;
            return Ok(parsed);
        }
    }
    Ok(LintsTomlFile::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_user_toml_yields_catalog_defaults() {
        let cfg = LintsConfig::from_catalog_defaults();
        // Catalog is empty until builtins/registry.rs ships; the loader
        // should at least walk to completion with no errors.
        assert!(cfg.config_errors.is_empty());
    }

    #[test]
    fn overlay_merges_nested_tables() {
        let mut dst: toml::Table = "[a]\nx = 1\n[a.b]\ny = 2\n".parse().unwrap();
        let src: toml::Table = "[a.b]\ny = 3\nz = 4\n".parse().unwrap();
        overlay(&mut dst, &src);
        let a = dst.get("a").unwrap().as_table().unwrap();
        let b = a.get("b").unwrap().as_table().unwrap();
        assert_eq!(b.get("y").unwrap().as_integer().unwrap(), 3);
        assert_eq!(b.get("z").unwrap().as_integer().unwrap(), 4);
        // Sibling keys preserved.
        assert_eq!(a.get("x").unwrap().as_integer().unwrap(), 1);
    }

    #[test]
    fn lint_filter_drops_unmatched_entries() {
        let overrides = OverrideCascade {
            lint_filter: Some(vec!["nonexistent-lint".to_string()]),
            ..Default::default()
        };
        let cfg = LintsConfig::from_inputs(LintsTomlFile::default(), overrides);
        assert!(cfg.entries.is_empty());
    }

    #[test]
    fn load_from_missing_root_returns_catalog_defaults() {
        let tmp = std::env::temp_dir().join("mockspace_test_lints_load");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let cfg = LintsConfig::load(&tmp, OverrideCascade::default()).unwrap();
        assert!(cfg.config_errors.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_reads_lints_toml() {
        let tmp = std::env::temp_dir().join("mockspace_test_lints_load_real");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("lints.toml"),
            "[lints.no-such-lint.config]\ntokens = []\n",
        )
        .unwrap();
        let cfg = LintsConfig::load(&tmp, OverrideCascade::default()).unwrap();
        // No registered entries means no instantiated lints from the file,
        // but no parse errors either.
        assert!(cfg.config_errors.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
