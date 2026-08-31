//! Cascade visualiser (#540).
//!
//! Walks the five-level cascade for a named lint and records each layer's
//! contribution to `config` and `scope`. The output is a structured
//! [`ExplainReport`] that callers (cargo mock subcommand at #560, future
//! diagnostic UIs, integration tests) consume directly, plus a `Display`
//! impl that renders the cascade-walkthrough format from
//! `mock/research/202605220500_lint-preset-infrastructure.md`.
//!
//! # Cascade layers
//!
//! Per the same memo + the `config_loader` module doc:
//!
//! 1. Catalog defaults (`CatalogEntry::default_config` / `default_scope`).
//! 2. Preset chain (innermost-first), resolved through a [`PresetSource`]
//!    from the per-lint `extends` field.
//! 3. Workspace defaults (`[defaults]` block in `lints.toml`).
//! 4. Per-lint TOML (`[lints.<name>.config]` and `[lints.<name>.scope]`).
//! 5. CLI overrides (`--scope`, `--severity-override`).
//!
//! Severity walks a separate channel (`LintCfgStore::resolve_severity`)
//! that does not pass through the cascade implemented here. The explain
//! command surfaces the catalog's default severity at layer 1 and notes
//! the gap; preset.severity / per-lint TOML severity surface once #566
//! lands.

use std::collections::HashSet;
use std::fmt;

use mockspace_config::{PresetFile, PresetResolveError, PresetSource};
use mockspace_core::lint::Severity;

use crate::catalog::{CatalogEntry, find_entry};
use crate::config_loader::{LintsTomlFile, OverrideCascade};
use crate::errors::{ConfigError, ConfigErrorKind};
use crate::preset_source::parse_extends;

// =========================================================================
// Output types.
// =========================================================================

/// One layer's contribution to the cascade walk. Each layer's entries
/// list the top-level keys it set on `config` and `scope`, in
/// alphabetical order for stable rendering.
#[derive(Debug, Clone)]
pub struct LayerContribution {
    /// Human-readable layer name (`"Layer 1: catalog defaults"`).
    pub label:  String,
    /// Optional source hint (`"mock://@/export/lint-preset/no-heap"`,
    /// `"lints.toml"`, etc.).
    pub source: Option<String>,
    /// Top-level config entries this layer set. Format `(field_path,
    /// value)`. `field_path` is just the top-level key today; nested
    /// table paths surface in a follow-up if explain UX needs them.
    pub config: Vec<(String, toml::Value)>,
    /// Top-level scope entries this layer set.
    pub scope:  Vec<(String, toml::Value)>,
}

/// The "Final" section's per-field resolution: which value won, which
/// layer set it.
#[derive(Debug, Clone)]
pub struct FinalEntry {
    /// `"config.forbidden"` or `"scope.crates"`.
    pub field_path:    String,
    pub value:         toml::Value,
    /// 1-indexed layer that contributed the winning value.
    pub winning_layer: usize,
    /// `label` of the winning layer for direct rendering.
    pub winning_label: String,
}

/// Full cascade explanation for one lint.
#[derive(Debug, Clone)]
pub struct ExplainReport {
    /// Lint name resolved against the catalog.
    pub lint_name:        String,
    /// `CatalogEntry::kind`, the primitive backing the lint.
    pub primitive_kind:   String,
    /// Per-layer contributions in cascade order.
    pub layers:           Vec<LayerContribution>,
    /// Resolved final value per top-level config + scope key.
    pub final_values:     Vec<FinalEntry>,
    /// Default severity from the catalog. Severity overlays from
    /// presets / per-lint TOML are not yet plumbed (#566); when they
    /// land, this struct grows a per-layer severity contribution
    /// list. Today only the catalog default is surfaced so the
    /// renderer still shows a "Severity:" line.
    pub catalog_severity: mockspace_core::lint::GateSeverity,
}

// =========================================================================
// Errors.
// =========================================================================

/// Failure produced while explaining a lint. Distinguishes "lint not
/// in catalog" from preset / cascade resolution errors (which surface
/// the same `ConfigError` shape that `instantiate_with_cascade` uses,
/// so call-sites can render the same diagnostic).
#[derive(Debug)]
pub enum ExplainError {
    LintNotFound {
        name: String,
    },
    Config(ConfigError),
}

impl fmt::Display for ExplainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LintNotFound {
                name,
            } => {
                write!(
                    f,
                    "lint `{name}` not found in the registered catalog; \
                 check the spelling or ensure the lint pack is linked"
                )
            },
            Self::Config(c) => write!(f, "{c}"),
        }
    }
}

impl std::error::Error for ExplainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(c) => Some(c),
            _ => None,
        }
    }
}

impl From<ConfigError> for ExplainError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

// =========================================================================
// Entry point.
// =========================================================================

/// Walk the cascade for `lint_name` and produce a structured
/// [`ExplainReport`]. Mirrors the layer order in `instantiate_with_cascade`
/// without invoking the lint's `instantiate` constructor; the report is
/// a read-only view onto the merged tables and the path that produced
/// them.
pub fn explain_lint(
    lint_name: &str,
    user_toml: &LintsTomlFile,
    overrides: &OverrideCascade,
    preset_source: &dyn PresetSource,
) -> Result<ExplainReport, ExplainError> {
    let entry = find_entry(lint_name).ok_or_else(|| {
        ExplainError::LintNotFound {
            name: lint_name.to_string(),
        }
    })?;
    explain_with_entry(entry, user_toml, overrides, preset_source)
}

/// Test-friendly variant that takes the catalog entry directly. Same
/// behaviour as [`explain_lint`] but bypasses the inventory lookup so
/// tests can construct a stub entry and exercise the cascade walk
/// without registering it globally.
pub fn explain_with_entry(
    entry: &CatalogEntry,
    user_toml: &LintsTomlFile,
    overrides: &OverrideCascade,
    preset_source: &dyn PresetSource,
) -> Result<ExplainReport, ExplainError> {
    let mut report = ExplainReport {
        lint_name:        entry.name.to_string(),
        primitive_kind:   entry.kind.to_string(),
        layers:           Vec::new(),
        final_values:     Vec::new(),
        catalog_severity: entry.default_severity,
    };

    let mut merged_config = parse_toml_block(entry.default_config, entry.name, "default_config")?;
    let mut merged_scope = parse_toml_block(entry.default_scope, entry.name, "default_scope")?;
    let mut config_provenance: Vec<(String, usize)> = vec![]; // (top_level_key, layer_idx)
    let mut scope_provenance: Vec<(String, usize)> = vec![];

    // Layer 1: catalog defaults.
    let layer1 = LayerContribution {
        label:  "Layer 1: catalog defaults".to_string(),
        source: Some(format!(
            "catalog entry `{}` (primitive `{}`)",
            entry.name, entry.kind
        )),
        config: table_to_sorted_entries(&merged_config),
        scope:  table_to_sorted_entries(&merged_scope),
    };
    for (k, _) in &layer1.config {
        config_provenance.push((k.clone(), 1));
    }
    for (k, _) in &layer1.scope {
        scope_provenance.push((k.clone(), 1));
    }
    report.layers.push(layer1);

    // Layer 2: preset chain.
    let user_block = user_toml.lints.get(entry.name);
    let mut layer2 = LayerContribution {
        label:  "Layer 2: preset chain".to_string(),
        source: None,
        config: Vec::new(),
        scope:  Vec::new(),
    };
    if let Some(block) = user_block {
        if let Some(extends_ref) = parse_extends(block.get("extends")).map_err(|e| {
            ConfigError {
                lint_name:       entry.name.to_string(),
                field_path:      "extends".to_string(),
                kind:            ConfigErrorKind::InvalidValue,
                message:         format!("{e}"),
                source_location: None,
            }
        })? {
            let chain = mockspace_config::resolve_preset_chain(&extends_ref, preset_source)
                .map_err(|e: PresetResolveError| {
                    ConfigError {
                        lint_name:       entry.name.to_string(),
                        field_path:      "extends".to_string(),
                        kind:            ConfigErrorKind::InvalidValue,
                        message:         format!("{e}"),
                        source_location: None,
                    }
                })?;
            layer2.source = Some(describe_chain(&chain));
            for preset in &chain {
                if !preset.config.is_empty() {
                    let overlay_table: toml::Table = preset
                        .config
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    record_overlay(
                        &mut merged_config,
                        &overlay_table,
                        2,
                        &mut config_provenance,
                    );
                    for (k, v) in &overlay_table {
                        layer2.config.push((k.clone(), v.clone()));
                    }
                }
                if !preset.scope.is_empty() {
                    let overlay_table: toml::Table = preset
                        .scope
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    record_overlay(&mut merged_scope, &overlay_table, 2, &mut scope_provenance);
                    for (k, v) in &overlay_table {
                        layer2.scope.push((k.clone(), v.clone()));
                    }
                }
            }
            // Dedup-and-sort layer2 entries so the renderer prints each
            // top-level key once with the value the chain settled on
            // (the outermost preset's overlay wins, mirroring
            // record_overlay's last-write semantics).
            layer2.config = dedup_keep_last(layer2.config);
            layer2.scope = dedup_keep_last(layer2.scope);
        }
    }
    report.layers.push(layer2);

    // Layer 3: workspace defaults.
    let workspace_defaults = user_toml.defaults.clone().unwrap_or_default();
    let layer3 = LayerContribution {
        label:  "Layer 3: workspace defaults".to_string(),
        source: if workspace_defaults.is_empty() {
            None
        } else {
            Some("`[defaults]` in lints.toml".to_string())
        },
        config: table_to_sorted_entries(&workspace_defaults),
        scope:  Vec::new(),
    };
    if !workspace_defaults.is_empty() {
        record_overlay(
            &mut merged_config,
            &workspace_defaults,
            3,
            &mut config_provenance,
        );
    }
    report.layers.push(layer3);

    // Layer 4: per-lint TOML.
    let mut layer4 = LayerContribution {
        label:  "Layer 4: per-lint TOML".to_string(),
        source: user_block.map(|_| format!("`[lints.{}]`", entry.name)),
        config: Vec::new(),
        scope:  Vec::new(),
    };
    if let Some(block) = user_block {
        if let Some(toml::Value::Table(t)) = block.get("config") {
            layer4.config = table_to_sorted_entries(t);
            record_overlay(&mut merged_config, t, 4, &mut config_provenance);
        }
        if let Some(toml::Value::Table(t)) = block.get("scope") {
            layer4.scope = table_to_sorted_entries(t);
            record_overlay(&mut merged_scope, t, 4, &mut scope_provenance);
        }
    }
    report.layers.push(layer4);

    // Layer 5: CLI overrides. The scope intersection mirrors what
    // `instantiate_with_cascade` applies at the same layer; severity
    // overrides walk a separate `LintCfgStore` channel and are noted
    // in the rendered output but not applied to the cascade here.
    let mut layer5 = LayerContribution {
        label:  "Layer 5: CLI overrides".to_string(),
        source: None,
        config: Vec::new(),
        scope:  Vec::new(),
    };
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
        let cli_set: HashSet<String> = overrides.scope_intersection.iter().cloned().collect();
        let intersected: Vec<toml::Value> =
            if existing_set.is_empty() || existing.iter().any(|v| v.as_str() == Some("*")) {
                cli_set.iter().cloned().map(toml::Value::String).collect()
            } else {
                existing_set
                    .intersection(&cli_set)
                    .cloned()
                    .map(toml::Value::String)
                    .collect()
            };
        let intersected_value = toml::Value::Array(intersected);
        merged_scope.insert("crates".to_string(), intersected_value.clone());
        layer5.source = Some(format!(
            "`--scope` intersection: [{}]",
            overrides.scope_intersection.join(", ")
        ));
        layer5.scope.push(("crates".to_string(), intersected_value));
        // Update provenance so the Final section attributes
        // `scope.crates` to layer 5 rather than to whichever layer
        // last wrote it pre-intersection.
        scope_provenance.retain(|(existing, _)| existing != "crates");
        scope_provenance.push(("crates".to_string(), 5));
    }
    // Surface severity overrides as an inert note so the renderer
    // can mention them even though the cascade does not consume them
    // here. The actual application path lives on `LintCfgStore`.
    if !overrides.severity_overrides.is_empty() {
        let names: Vec<String> = overrides
            .severity_overrides
            .iter()
            .map(|(k, v)| format!("{k}={}", severity_label(*v)))
            .collect();
        let combined = match &layer5.source {
            Some(existing) => {
                format!(
                    "{existing}; `--severity-override` (not applied here, see #566): {}",
                    names.join(", ")
                )
            },
            None => {
                format!(
                    "`--severity-override` (not applied here, see #566): {}",
                    names.join(", ")
                )
            },
        };
        layer5.source = Some(combined);
    }
    report.layers.push(layer5);

    // Compute final entries from the resolved tables + provenance.
    report.final_values = compose_finals(
        &merged_config,
        &merged_scope,
        &config_provenance,
        &scope_provenance,
        &report.layers,
    );

    Ok(report)
}

// =========================================================================
// Internal helpers.
// =========================================================================

fn parse_toml_block(
    raw: &str,
    lint_name: &str,
    field_path: &str,
) -> Result<toml::Table, ConfigError> {
    raw.parse().map_err(|e: toml::de::Error| {
        ConfigError {
            lint_name:       lint_name.to_string(),
            field_path:      field_path.to_string(),
            kind:            ConfigErrorKind::InvalidValue,
            message:         format!("{field_path} did not parse: {e}"),
            source_location: None,
        }
    })
}

fn table_to_sorted_entries(table: &toml::Table) -> Vec<(String, toml::Value)> {
    let mut entries: Vec<(String, toml::Value)> =
        table.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn record_overlay(
    dst: &mut toml::Table,
    src: &toml::Table,
    layer_idx: usize,
    provenance: &mut Vec<(String, usize)>,
) {
    for (k, v) in src {
        dst.insert(k.clone(), v.clone());
        // Update provenance: each key's most-recent layer wins,
        // so we re-record on each overlay touch.
        provenance.retain(|(existing, _)| existing != k);
        provenance.push((k.clone(), layer_idx));
    }
}

fn describe_chain(chain: &[PresetFile]) -> String {
    // The resolver hands the chain back with `chain[0]` = deepest
    // extends ancestor and `chain[last]` = the starting preset (the
    // consumer's direct reference). For the explain renderer, the
    // starting preset is the most useful label; the depth chain
    // shows it after the arrow when relevant.
    let names: Vec<String> = chain.iter().map(|p| p.name.clone()).collect();
    if names.is_empty() {
        "(empty chain)".to_string()
    } else {
        format!(
            "preset chain (deepest extends -> starting preset): {}",
            names.join(" -> ")
        )
    }
}

/// Deduplicate by key (top-level entry), keeping the last value
/// observed per key. The chain walks deepest-extends first; the
/// starting preset (the consumer's direct reference) lands last,
/// so keeping the last occurrence reflects the resolved value that
/// the starting preset established (winning on field overlap).
fn dedup_keep_last(entries: Vec<(String, toml::Value)>) -> Vec<(String, toml::Value)> {
    use std::collections::BTreeMap;
    // BTreeMap preserves alphabetical order on the way out and lets a
    // re-insertion overwrite the prior value.
    let mut map: BTreeMap<String, toml::Value> = BTreeMap::new();
    for (k, v) in entries {
        map.insert(k, v);
    }
    map.into_iter().collect()
}

fn compose_finals(
    config: &toml::Table,
    scope: &toml::Table,
    config_provenance: &[(String, usize)],
    scope_provenance: &[(String, usize)],
    layers: &[LayerContribution],
) -> Vec<FinalEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (key, value) in config {
        let winning = config_provenance
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, idx)| *idx)
            .unwrap_or(1);
        let winning_label = layers
            .get(winning.saturating_sub(1))
            .map(|l| l.label.clone())
            .unwrap_or_default();
        let path = format!("config.{key}");
        if seen.insert(path.clone()) {
            out.push(FinalEntry {
                field_path: path,
                value: value.clone(),
                winning_layer: winning,
                winning_label,
            });
        }
    }
    for (key, value) in scope {
        let winning = scope_provenance
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, idx)| *idx)
            .unwrap_or(1);
        let winning_label = layers
            .get(winning.saturating_sub(1))
            .map(|l| l.label.clone())
            .unwrap_or_default();
        let path = format!("scope.{key}");
        if seen.insert(path.clone()) {
            out.push(FinalEntry {
                field_path: path,
                value: value.clone(),
                winning_layer: winning,
                winning_label,
            });
        }
    }
    out.sort_by(|a, b| a.field_path.cmp(&b.field_path));
    out
}

// =========================================================================
// Display rendering.
// =========================================================================

impl fmt::Display for ExplainReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Lint: {} (primitive: {})",
            self.lint_name, self.primitive_kind
        )?;
        for layer in &self.layers {
            writeln!(f)?;
            writeln!(f, "  {}", layer.label)?;
            if let Some(src) = &layer.source {
                writeln!(f, "    source: {src}")?;
            }
            if layer.config.is_empty() && layer.scope.is_empty() {
                writeln!(f, "    (no overrides at this layer)")?;
                continue;
            }
            for (k, v) in &layer.config {
                writeln!(f, "    config.{k} = {}", render_value(v))?;
            }
            for (k, v) in &layer.scope {
                writeln!(f, "    scope.{k} = {}", render_value(v))?;
            }
        }
        writeln!(f)?;
        writeln!(f, "Severity (catalog default):")?;
        writeln!(f, "  commit = {:?}", self.catalog_severity.commit)?;
        writeln!(f, "  build  = {:?}", self.catalog_severity.build)?;
        writeln!(f, "  push   = {:?}", self.catalog_severity.push)?;
        writeln!(
            f,
            "  (preset / per-lint / CLI severity overlays not yet plumbed; tracked at #566)"
        )?;
        writeln!(f)?;
        writeln!(f, "Final:")?;
        if self.final_values.is_empty() {
            writeln!(f, "  (no fields set)")?;
        }
        for entry in &self.final_values {
            writeln!(
                f,
                "  {} = {} ({})",
                entry.field_path,
                render_value(&entry.value),
                entry.winning_label
            )?;
        }
        Ok(())
    }
}

fn render_value(v: &toml::Value) -> String {
    // Stable, compact rendering. `toml::Value::to_string` produces
    // the inline TOML form for arrays and tables, which is what
    // explain readers want.
    match v {
        toml::Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Skip => "skip",
        Severity::Off => "off",
        Severity::Hint => "hint",
        Severity::Info => "info",
        Severity::Warn => "warn",
        Severity::Error => "error",
    }
}

// =========================================================================
// Tests.
// =========================================================================

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use mockspace_config::{PresetFile, PresetRef, PresetResolveError};
    use mockspace_core::lint::{GateSeverity, Severity};

    use super::*;
    use crate::config_loader::LintsTomlFile;
    use crate::lint::Lint;

    // ----- stub plumbing ----------------------------------------------------

    struct NoOpLint;

    impl Lint for NoOpLint {
        fn name(&self) -> &'static str {
            "explained"
        }

        fn description(&self) -> &'static str {
            "explain test stub"
        }

        fn default_severity(&self) -> GateSeverity {
            GateSeverity::uniform(Severity::Warn)
        }
    }

    fn stub_instantiate(
        _config: &toml::Table,
        _scope: &toml::Table,
    ) -> Result<Box<dyn Lint>, ConfigError> {
        Ok(Box::new(NoOpLint))
    }

    fn stub_entry() -> CatalogEntry {
        CatalogEntry {
            name:             "explained",
            description:      "test stub",
            kind:             "test-explained",
            default_config:   "level = \"catalog\"\nflag = false\n",
            default_scope:    "category = \"catalog\"\n",
            default_severity: GateSeverity::uniform(Severity::Warn),
            default_impact:   None,
            default_category: None,
            doc_url:          None,
            mode:             crate::lint::LintMode::PerDocument,
            staging_aware:    false,
            editor_skip:      false,
            instantiate:      stub_instantiate,
            finding_kinds:    &[],
        }
    }

    struct InMemorySource {
        presets: BTreeMap<(String, String), PresetFile>,
    }

    impl InMemorySource {
        fn new() -> Self {
            Self {
                presets: BTreeMap::new(),
            }
        }

        fn insert(mut self, host: &str, preset: PresetFile) -> Self {
            self.presets
                .insert((host.to_string(), preset.name.clone()), preset);
            self
        }
    }

    impl PresetSource for InMemorySource {
        fn resolve(&self, preset_ref: &PresetRef) -> Result<PresetFile, PresetResolveError> {
            self.presets
                .get(&(preset_ref.host.clone(), preset_ref.name.clone()))
                .cloned()
                .ok_or_else(|| {
                    PresetResolveError::NotFound {
                        host: preset_ref.host.clone(),
                        name: preset_ref.name.clone(),
                    }
                })
        }
    }

    fn preset(name: &str, extends: Option<&str>, config: &str, scope: &str) -> PresetFile {
        let config: BTreeMap<String, toml::Value> =
            config.parse::<toml::Table>().unwrap().into_iter().collect();
        let scope: BTreeMap<String, toml::Value> =
            scope.parse::<toml::Table>().unwrap().into_iter().collect();
        PresetFile {
            schema_version: "1.0".to_string(),
            name: name.to_string(),
            primitive: "test-explained".to_string(),
            description: None,
            extends: extends.map(String::from),
            config,
            severity: Default::default(),
            scope,
        }
    }

    // ----- happy paths ------------------------------------------------------

    #[test]
    fn no_user_toml_yields_only_catalog_layer_contributions() {
        let entry = stub_entry();
        let report = explain_with_entry(
            &entry,
            &LintsTomlFile::default(),
            &OverrideCascade::default(),
            &InMemorySource::new(),
        )
        .unwrap();
        assert_eq!(report.lint_name, "explained");
        assert_eq!(report.primitive_kind, "test-explained");
        assert_eq!(report.layers.len(), 5);
        // Layer 1 carries the catalog defaults.
        let l1 = &report.layers[0];
        assert!(l1.config.iter().any(|(k, _)| k == "level"));
        assert!(l1.scope.iter().any(|(k, _)| k == "category"));
        // Layers 2-5 should have no contribution since no preset, no
        // workspace defaults, no per-lint TOML, no CLI overrides.
        for layer in &report.layers[1 ..] {
            assert!(layer.config.is_empty());
            assert!(layer.scope.is_empty());
        }
        // Finals attribute to layer 1.
        for fv in &report.final_values {
            assert_eq!(fv.winning_layer, 1, "{}: catalog should win", fv.field_path);
        }
    }

    #[test]
    fn preset_overlay_attributed_to_layer_two() {
        let entry = stub_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset("base", None, "level = \"preset\"\nadded = 1\n", ""),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::base\"\n".parse().unwrap();
        user_lints.insert("explained".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: None,
        };
        let report =
            explain_with_entry(&entry, &user_toml, &OverrideCascade::default(), &source).unwrap();
        // Layer 2 carries the preset's config keys.
        let l2 = &report.layers[1];
        assert_eq!(l2.source.as_deref().unwrap_or("").contains("base"), true);
        let l2_config_keys: Vec<&str> = l2.config.iter().map(|(k, _)| k.as_str()).collect();
        assert!(l2_config_keys.contains(&"level"));
        assert!(l2_config_keys.contains(&"added"));
        // `level` final winner is layer 2; `added` came from layer 2;
        // `flag` retained from layer 1.
        let final_map: HashMap<&str, usize> = report
            .final_values
            .iter()
            .map(|fv| (fv.field_path.as_str(), fv.winning_layer))
            .collect();
        assert_eq!(final_map.get("config.level"), Some(&2));
        assert_eq!(final_map.get("config.added"), Some(&2));
        assert_eq!(final_map.get("config.flag"), Some(&1));
    }

    #[test]
    fn per_lint_toml_overlay_attributed_to_layer_four() {
        let entry = stub_entry();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "[config]\nlevel = \"user\"\n".parse().unwrap();
        user_lints.insert("explained".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: None,
        };
        let report = explain_with_entry(
            &entry,
            &user_toml,
            &OverrideCascade::default(),
            &InMemorySource::new(),
        )
        .unwrap();
        let final_map: HashMap<&str, usize> = report
            .final_values
            .iter()
            .map(|fv| (fv.field_path.as_str(), fv.winning_layer))
            .collect();
        assert_eq!(final_map.get("config.level"), Some(&4));
    }

    #[test]
    fn workspace_defaults_overlay_attributed_to_layer_three() {
        let entry = stub_entry();
        let workspace_defaults: toml::Table = "flag = true\n".parse().unwrap();
        let user_toml = LintsTomlFile {
            lints:    HashMap::new(),
            defaults: Some(workspace_defaults),
        };
        let report = explain_with_entry(
            &entry,
            &user_toml,
            &OverrideCascade::default(),
            &InMemorySource::new(),
        )
        .unwrap();
        let final_map: HashMap<&str, usize> = report
            .final_values
            .iter()
            .map(|fv| (fv.field_path.as_str(), fv.winning_layer))
            .collect();
        assert_eq!(final_map.get("config.flag"), Some(&3));
        // `level` stayed at layer 1.
        assert_eq!(final_map.get("config.level"), Some(&1));
    }

    #[test]
    fn full_cascade_resolves_in_documented_order() {
        let entry = stub_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset("stacked", None, "level = \"preset\"\nflag = false\n", ""),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        // User TOML overrides config.level via per-lint block + sets workspace defaults too.
        let user_block: toml::Table =
            "extends = \"mockspace::stacked\"\n[config]\nlevel = \"user\"\n"
                .parse()
                .unwrap();
        user_lints.insert("explained".to_string(), user_block);
        let workspace_defaults: toml::Table = "flag = true\n".parse().unwrap();
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: Some(workspace_defaults),
        };
        let report =
            explain_with_entry(&entry, &user_toml, &OverrideCascade::default(), &source).unwrap();
        let final_map: HashMap<&str, usize> = report
            .final_values
            .iter()
            .map(|fv| (fv.field_path.as_str(), fv.winning_layer))
            .collect();
        // config.level: catalog -> preset -> (workspace untouched) -> per-lint -> CLI
        // Per-lint wins (layer 4).
        assert_eq!(final_map.get("config.level"), Some(&4));
        // config.flag: catalog (false) -> preset (false) -> workspace defaults (true) -> not in per-lint
        // Workspace defaults win (layer 3).
        assert_eq!(final_map.get("config.flag"), Some(&3));
    }

    // ----- error paths ------------------------------------------------------

    #[test]
    fn missing_extends_target_surfaces_as_config_error() {
        let entry = stub_entry();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::absent\"\n".parse().unwrap();
        user_lints.insert("explained".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: None,
        };
        match explain_with_entry(
            &entry,
            &user_toml,
            &OverrideCascade::default(),
            &InMemorySource::new(),
        ) {
            Err(ExplainError::Config(c)) => {
                assert_eq!(c.field_path, "extends");
                assert!(c.message.contains("absent"));
            },
            other => panic!("expected ExplainError::Config, got {other:?}"),
        }
    }

    #[test]
    fn lint_not_in_catalog_yields_lint_not_found() {
        let user_toml = LintsTomlFile::default();
        match explain_lint(
            "definitely-not-a-real-lint",
            &user_toml,
            &OverrideCascade::default(),
            &InMemorySource::new(),
        ) {
            Err(ExplainError::LintNotFound {
                name,
            }) => {
                assert_eq!(name, "definitely-not-a-real-lint");
            },
            other => panic!("expected LintNotFound, got {other:?}"),
        }
    }

    // ----- Display rendering ------------------------------------------------

    #[test]
    fn display_renders_header_layers_and_finals() {
        let entry = stub_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset("base", None, "level = \"preset\"\n", ""),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        user_lints.insert(
            "explained".to_string(),
            "extends = \"mockspace::base\"\n".parse().unwrap(),
        );
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: None,
        };
        let report =
            explain_with_entry(&entry, &user_toml, &OverrideCascade::default(), &source).unwrap();
        let rendered = format!("{report}");
        // Headers and section markers all present.
        assert!(rendered.contains("Lint: explained (primitive: test-explained)"));
        assert!(rendered.contains("Layer 1: catalog defaults"));
        assert!(rendered.contains("Layer 2: preset chain"));
        assert!(rendered.contains("Layer 3: workspace defaults"));
        assert!(rendered.contains("Layer 4: per-lint TOML"));
        assert!(rendered.contains("Layer 5: CLI overrides"));
        assert!(rendered.contains("Severity (catalog default):"));
        assert!(rendered.contains("#566"));
        assert!(rendered.contains("Final:"));
        assert!(rendered.contains("config.level"));
        // Preset chain source line names the preset.
        assert!(rendered.contains("base"));
    }

    // ----- CLI override layer 5 --------------------------------------------

    #[test]
    fn cli_scope_intersection_applies_at_layer_five_and_wins_provenance() {
        let entry = stub_entry();
        // Per-lint TOML sets scope.crates = ["a", "b", "c"]; CLI
        // intersection narrows to ["b", "c"] (and a non-matching "z").
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "[scope]\ncrates = [\"a\", \"b\", \"c\"]\n".parse().unwrap();
        user_lints.insert("explained".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints:    user_lints,
            defaults: None,
        };
        let overrides = OverrideCascade {
            scope_intersection: vec!["b".to_string(), "c".to_string(), "z".to_string()],
            ..Default::default()
        };
        let report =
            explain_with_entry(&entry, &user_toml, &overrides, &InMemorySource::new()).unwrap();
        // Layer 5 surfaces the intersection in source + scope.
        let l5 = &report.layers[4];
        assert!(l5.source.as_deref().unwrap_or("").contains("--scope"));
        assert!(l5.scope.iter().any(|(k, _)| k == "crates"));
        // Provenance: scope.crates final attributes to layer 5.
        let final_map: HashMap<&str, usize> = report
            .final_values
            .iter()
            .map(|fv| (fv.field_path.as_str(), fv.winning_layer))
            .collect();
        assert_eq!(final_map.get("scope.crates"), Some(&5));
        // The intersected array contains "b" and "c" but not "a" or "z".
        let final_value = report
            .final_values
            .iter()
            .find(|fv| fv.field_path == "scope.crates")
            .unwrap();
        let arr = final_value.value.as_array().unwrap();
        let str_values: HashSet<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        assert!(str_values.contains("b"));
        assert!(str_values.contains("c"));
        assert!(!str_values.contains("a"));
        assert!(!str_values.contains("z"));
    }

    #[test]
    fn cli_severity_override_surfaces_as_note_without_applying() {
        let entry = stub_entry();
        let mut severity_overrides: HashMap<String, Severity> = HashMap::new();
        severity_overrides.insert("explained".to_string(), Severity::Error);
        let overrides = OverrideCascade {
            severity_overrides,
            ..Default::default()
        };
        let report = explain_with_entry(
            &entry,
            &LintsTomlFile::default(),
            &overrides,
            &InMemorySource::new(),
        )
        .unwrap();
        let l5 = &report.layers[4];
        let source = l5.source.as_deref().unwrap_or("");
        assert!(source.contains("severity-override"));
        assert!(source.contains("#566"));
        assert!(source.contains("explained=error"));
    }

    #[test]
    fn display_renders_empty_user_state_with_only_catalog_finals() {
        let entry = stub_entry();
        let report = explain_with_entry(
            &entry,
            &LintsTomlFile::default(),
            &OverrideCascade::default(),
            &InMemorySource::new(),
        )
        .unwrap();
        let rendered = format!("{report}");
        // Layers 2-5 mark themselves as having no overrides.
        let no_override_count = rendered.matches("(no overrides at this layer)").count();
        assert_eq!(no_override_count, 4);
    }
}
