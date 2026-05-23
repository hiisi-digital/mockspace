//! TOML configuration loader and override cascade.
//!
//! Per schema design memo §11 + preset infrastructure memo at
//! `mock/research/202605220500_lint-preset-infrastructure.md`. Cascade
//! precedence, lowest-to-highest:
//!
//! 1. `CatalogEntry::default_config` + `default_scope` + `default_severity`.
//! 2. Preset chain (resolved through [`mockspace_config::PresetSource`]
//!    from the per-lint `extends = "<host>::<name>"` shorthand, walked
//!    innermost-first).
//! 3. Workspace-level `[lints]` defaults.
//! 4. Per-lint `[lints.<name>]` blocks in `lints.toml`.
//! 5. CLI overrides (`--scope`, `--lint`, `--severity-override`).
//!
//! The two-channel return shape (`(entries, config_errors)`) keeps source
//! findings and configuration faults separate per §9.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::catalog::CatalogEntry;
use crate::errors::{ConfigError, ConfigErrorKind, LoadError, StartupWarning};
use crate::lint::{Lint, LintMode};
use crate::preset_source::{parse_extends, FirstPartyPresetSource};
use mockspace_config::{resolve_preset_chain, PresetFile, PresetResolveError, PresetSource};

/// Prop names with this prefix are reserved as the first-party namespace.
/// Collisions among multiple lints declaring the same `mockspace::`-prefixed
/// prop name are silent (assumed coordinated within one pack); unqualified
/// collisions raise [`StartupWarning::PropNameConflict`].
///
/// Note: a literal `"mockspace::"` (the prefix on its own, with an empty
/// local segment) starts_with itself and is therefore silenced. That input
/// is a declaring lint's bug rather than the detector's concern; the
/// surrounding directive-style-consistency lint (#548) is the right place
/// to surface empty local segments.
const FIRST_PARTY_PROP_NS: &str = "mockspace::";

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
    /// Severity resolved through the cascade (preset chain plus user
    /// TOML plus CLI overrides, walked over the lint's catalog default).
    /// `None` means no overlay touched the severity at any cascade
    /// layer and the engine should fall back to `lint.default_severity()`.
    /// Per the cascade memo and task #566, the layers compose:
    /// catalog default (engine fallback) -> preset chain (innermost
    /// first) -> user TOML per-lint -> CLI severity_overrides.
    pub resolved_severity: Option<mockspace_core::lint::GateSeverity>,
}

/// Parsed configuration plus instantiated lints, ready for engine
/// dispatch. The split between `entries`, `config_errors`, and
/// `startup_warnings` is the three-channel return shape: errors block
/// load, warnings surface non-fatal observations, entries are the
/// dispatch set.
pub struct LintsConfig {
    pub entries: Vec<InstantiatedLint>,
    pub config_errors: Vec<ConfigError>,
    /// Non-fatal observations made at engine assembly. Populated after
    /// `entries` is built; see [`detect_prop_name_conflicts`].
    pub startup_warnings: Vec<StartupWarning>,
}

impl std::fmt::Debug for LintsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LintsConfig")
            .field("entries", &self.entries.len())
            .field("config_errors", &self.config_errors.len())
            .field("startup_warnings", &self.startup_warnings.len())
            .finish()
    }
}

impl LintsConfig {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            config_errors: Vec::new(),
            startup_warnings: Vec::new(),
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
    pub fn load(workspace_root: &Path, overrides: OverrideCascade) -> Result<Self, LoadError> {
        let user_toml = find_and_read_lints_toml(workspace_root)?;
        Ok(Self::from_inputs(user_toml, overrides))
    }

    /// Direct cascade entry point: pass parsed user TOML + overrides.
    /// Uses the build.rs-embedded first-party preset table as the only
    /// resolution source. Test-friendly form; production callers use
    /// [`Self::load`].
    pub fn from_inputs(user_toml: LintsTomlFile, overrides: OverrideCascade) -> Self {
        let source = FirstPartyPresetSource::new();
        Self::from_inputs_with_source(user_toml, overrides, &source)
    }

    /// Like [`Self::from_inputs`] but with an explicit preset source.
    /// Tests inject a mock source carrying canned presets; production
    /// callers go through [`Self::from_inputs`] which binds to the
    /// embedded first-party table. A future external-preset chain
    /// loader composes additional sources behind this same surface.
    pub fn from_inputs_with_source(
        user_toml: LintsTomlFile,
        overrides: OverrideCascade,
        preset_source: &dyn PresetSource,
    ) -> Self {
        let mut entries = Vec::new();
        let mut config_errors = Vec::new();

        // Workspace-default block (low-precedence overlay over each lint
        // sub-table, applied at the engine merge step below).
        let workspace_defaults = user_toml.defaults.clone().unwrap_or_default();

        let mut catalog_names: HashSet<&str> = HashSet::new();

        for entry in crate::catalog::catalog_entries() {
            catalog_names.insert(entry.name);
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
                preset_source,
            ) {
                Ok(lint) => entries.push(lint),
                Err(e) => config_errors.push(e),
            }
        }

        // Synthesised path (#611): user [lints.<X>] blocks whose name
        // is NOT in the registered catalog are resolved through the
        // preset-as-catalog path. The block MUST carry `extends =
        // "<host>::<name>"`; the resolver looks up the preset chain
        // through `preset_source`, picks the anchor's primitive name,
        // looks up the corresponding `PrimitiveDescriptor`, and
        // synthesises an `InstantiatedLint` with the cascade math
        // shared with the entry path via `compute_cascade`.
        //
        // A block with no `extends` for an unregistered lint name
        // emits ConfigError::UnknownLint per the original v2 contract.
        for (lint_name, user_block) in &user_toml.lints {
            if catalog_names.contains(lint_name.as_str()) {
                continue;
            }
            if let Some(filter) = &overrides.lint_filter {
                if !filter.iter().any(|n| n == lint_name) {
                    continue;
                }
            }
            match synthesise_from_preset(
                lint_name,
                user_block,
                &workspace_defaults,
                &overrides,
                preset_source,
            ) {
                Ok(lint) => entries.push(lint),
                Err(e) => config_errors.push(e),
            }
        }

        let startup_warnings = detect_prop_name_conflicts(&entries);
        Self {
            entries,
            config_errors,
            startup_warnings,
        }
    }
}

// =========================================================================
// Startup namespace-conflict detection.
// =========================================================================

/// Walk the assembled lint set and emit a [`StartupWarning::PropNameConflict`]
/// for each prop name declared by two or more distinct lints unless the
/// name is prefixed [`FIRST_PARTY_PROP_NS`] (first-party namespace; treated
/// as one pack and silenced).
///
/// Per the `lint:prop` design memo at
/// `mock/research/202605220600_lint-provided-marker-directive.md`
/// § "Namespace handling: detect, do not require". The warning is
/// advisory; the engine continues to load. The future
/// `directive-style-consistency` lint (#548) catches the orthogonal
/// failure mode (a source uses a prop name no lint declares).
///
/// Lints listed in the warning are sorted and deduplicated so a single
/// lint declaring the same name twice does not self-conflict.
pub fn detect_prop_name_conflicts(entries: &[InstantiatedLint]) -> Vec<StartupWarning> {
    use std::collections::BTreeMap;

    let mut by_prop: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for entry in entries {
        let lint_name = entry.lint.name();
        for prop_name in entry.lint.declared_props() {
            by_prop.entry(*prop_name).or_default().push(lint_name);
        }
    }

    let mut warnings = Vec::new();
    for (prop_name, mut lints) in by_prop {
        if prop_name.starts_with(FIRST_PARTY_PROP_NS) {
            continue;
        }
        lints.sort_unstable();
        lints.dedup();
        if lints.len() < 2 {
            continue;
        }
        warnings.push(StartupWarning::PropNameConflict {
            prop_name: prop_name.to_string(),
            lints: lints.into_iter().map(String::from).collect(),
        });
    }
    warnings
}

// =========================================================================
// Per-entry cascade execution.
// =========================================================================

fn extract_only_staged(
    lint_name: &str,
    staging_aware: bool,
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
        if *only_staged && !staging_aware {
            return Err(ConfigError {
                lint_name: lint_name.to_string(),
                field_path: format!("gate.{gate_name}.only_staged"),
                kind: crate::errors::ConfigErrorKind::ContradictsCatalog,
                message: format!(
                    "lint `{lint_name}` is not staging_aware; `only_staged = true` is invalid here"
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
                    lint_name: lint_name.to_string(),
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

/// Inputs to the cascade-math pass, abstracted so both the catalog
/// entry path and the upcoming preset-as-catalog synthesis path
/// (#611 PR-3) can share the layer composition.
///
/// `layer1_*` come from whichever source establishes the cascade
/// floor: a `CatalogEntry::default_*` for the entry path, or a
/// preset file's own `config` / `scope` / `severity` for the
/// synthesised path. Higher cascade layers (preset chain, workspace
/// defaults, per-lint TOML, CLI overrides) are independent of which
/// floor the caller picked.
///
/// The floor tables are passed by value: `compute_cascade` consumes
/// them and applies higher-layer overlays in place. Callers parse or
/// clone the floor before calling; the struct's `&'a` lifetime
/// covers only the borrowed context (user_block, workspace_defaults,
/// overrides, preset_source).
pub(crate) struct CascadeInputs<'a> {
    pub lint_name: &'a str,
    pub layer1_config: toml::Table,
    pub layer1_scope: toml::Table,
    pub layer1_severity: mockspace_core::lint::GateSeverity,
    /// Optional per-lint user TOML block (`[lints.<name>]`). Carries
    /// `extends`, `config`, `scope`, `gate`.
    pub user_block: Option<&'a toml::Table>,
    pub workspace_defaults: &'a toml::Table,
    pub overrides: &'a OverrideCascade,
    pub preset_source: &'a dyn PresetSource,
}

/// Outputs of the cascade-math pass. The merged tables feed the
/// primitive's `instantiate_with`; the resolved chain feeds the
/// severity cascade and (PR-3) the synthesis path's primitive
/// mismatch check; `resolved_severity` is the final
/// post-cascade severity or `None` when no layer touched it.
pub(crate) struct CascadeOutput {
    pub merged_config: toml::Table,
    pub merged_scope: toml::Table,
    pub resolved_chain: Vec<PresetFile>,
    pub resolved_severity: Option<mockspace_core::lint::GateSeverity>,
}

/// Walk the five-layer cascade producing merged TOML tables + the
/// resolved severity + the preset chain. Pure function over the
/// inputs; no entry-specific assumptions beyond what
/// `CascadeInputs` carries.
///
/// Layers, deepest-first:
/// 1. `layer1_config` / `layer1_scope` (caller-provided floor).
/// 2. Preset chain resolved through `preset_source`, walked
///    innermost-first.
/// 3. `workspace_defaults`.
/// 4. `user_block.config` / `user_block.scope`.
/// 5. CLI `scope_intersection` (against `merged_scope.crates`) and
///    `severity_overrides`.
///
/// The severity cascade is composed from `layer1_severity` plus the
/// resolved preset chain plus `user_block.gate.*.severity` plus
/// `overrides.severity_overrides`. Unset axes inherit from the
/// deeper layer.
pub(crate) fn compute_cascade(
    inputs: CascadeInputs<'_>,
) -> Result<CascadeOutput, ConfigError> {
    let CascadeInputs {
        lint_name,
        mut layer1_config,
        mut layer1_scope,
        layer1_severity,
        user_block,
        workspace_defaults,
        overrides,
        preset_source,
    } = inputs;

    // Level 2: preset chain.
    let resolved_chain: Vec<PresetFile> = if let Some(block) = user_block {
        if let Some(extends_ref) = parse_extends(block.get("extends"))
            .map_err(|e| preset_error_to_config_error(lint_name, "extends", e))?
        {
            let chain = resolve_preset_chain(&extends_ref, preset_source)
                .map_err(|e| preset_error_to_config_error(lint_name, "extends", e))?;
            apply_preset_chain(&chain, &mut layer1_config, &mut layer1_scope);
            chain
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Level 3: workspace defaults (overlay where present).
    overlay(&mut layer1_config, workspace_defaults);

    // Level 4: per-lint user TOML config / scope. (Other user-block
    // keys like `gate` consult the resolved severity cascade or
    // get validated by the caller against entry-specific
    // metadata.)
    if let Some(block) = user_block {
        if let Some(toml::Value::Table(t)) = block.get("config") {
            overlay(&mut layer1_config, t);
        }
        if let Some(toml::Value::Table(t)) = block.get("scope") {
            overlay(&mut layer1_scope, t);
        }
    }

    // Level 5: CLI scope intersection.
    if !overrides.scope_intersection.is_empty() {
        let existing = layer1_scope
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
        layer1_scope.insert("crates".to_string(), toml::Value::Array(intersected));
    }

    let resolved_severity = resolve_severity_cascade(
        layer1_severity,
        &resolved_chain,
        user_block,
        &overrides.severity_overrides,
        lint_name,
    );

    Ok(CascadeOutput {
        merged_config: layer1_config,
        merged_scope: layer1_scope,
        resolved_chain,
        resolved_severity,
    })
}

/// Synthesise an `InstantiatedLint` from a preset reference for a
/// lint name that is not in the auto-registered catalog (#611).
///
/// Walks the cascade with layer-1 inputs drawn from the consumer's
/// directly-referenced preset (the "anchor"): the anchor's `config`
/// and `scope` populate the floor; the anchor's `severity` overlays
/// onto a uniform-Warn default for any gates the preset leaves
/// unspecified. compute_cascade then walks the rest of the chain,
/// workspace defaults, user TOML, and CLI overrides on top.
///
/// Errors:
/// - `extends` missing: `InvalidValue` with a message naming what
///   the consumer must add. This is the v2 contract for "lint name X
///   not in catalog and no preset reference exists".
/// - chain primitive mismatch: `ContradictsCatalog`; one chain
///   represents one lint's policy, so every preset in the chain
///   must point at the same primitive.
/// - unknown primitive (preset names a primitive not in
///   `PRIMITIVE_DESCRIPTORS`): `UnknownKind`.
fn synthesise_from_preset(
    lint_name: &str,
    user_block: &toml::Table,
    workspace_defaults: &toml::Table,
    overrides: &OverrideCascade,
    preset_source: &dyn PresetSource,
) -> Result<InstantiatedLint, ConfigError> {
    // Parse `extends`. Missing means the consumer wrote a [lints.X]
    // block for a name not in the catalog with no preset reference;
    // emit a structured error that names the required field.
    let extends_ref = match parse_extends(user_block.get("extends")) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Err(ConfigError {
                lint_name: lint_name.to_string(),
                field_path: "extends".to_string(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!(
                    "lint `{lint_name}` is not in the registered catalog; \
                     add `extends = \"<host>::<preset-name>\"` to reference \
                     a first-party or stack-lints preset"
                ),
                source_location: None,
            });
        }
        Err(e) => return Err(preset_error_to_config_error(lint_name, "extends", e)),
    };

    let chain = resolve_preset_chain(&extends_ref, preset_source)
        .map_err(|e| preset_error_to_config_error(lint_name, "extends", e))?;

    // The chain's last entry is the consumer's direct reference (the
    // "anchor"). Its primitive defines the synthesised lint's shape;
    // every other preset in the chain MUST agree on that primitive.
    let anchor = chain.last().ok_or_else(|| ConfigError {
        lint_name: lint_name.to_string(),
        field_path: "extends".to_string(),
        kind: ConfigErrorKind::InvalidValue,
        message: "preset chain resolved to an empty list".to_string(),
        source_location: None,
    })?;

    for preset in &chain {
        if preset.primitive != anchor.primitive {
            return Err(ConfigError {
                lint_name: lint_name.to_string(),
                field_path: format!("extends.chain.{}", preset.name),
                kind: ConfigErrorKind::ContradictsCatalog,
                message: format!(
                    "preset chain spans primitives: `{}` (anchor) vs `{}` (in chain via `{}`); \
                     one chain represents one lint's policy",
                    anchor.primitive, preset.primitive, preset.name
                ),
                source_location: None,
            });
        }
    }

    let descriptor = crate::builtins::primitives::find_descriptor(&anchor.primitive)
        .ok_or_else(|| ConfigError {
            lint_name: lint_name.to_string(),
            field_path: "extends".to_string(),
            kind: ConfigErrorKind::UnknownKind,
            message: format!(
                "preset `{}` names primitive `{}` which is not in the registered descriptor set",
                anchor.name, anchor.primitive
            ),
            source_location: None,
        })?;

    // Layer-1 floor: empty tables and uniform-Warn severity.
    // compute_cascade walks the full chain (which already includes the
    // anchor) innermost-first via its own `apply_preset_chain` +
    // `resolve_severity_cascade` calls. Passing the chain severities
    // here too would double-apply the overlay; idempotent under the
    // current per-axis `Some`-replace semantics but a correctness
    // landmine if `overlay_gate_severities` ever becomes
    // non-idempotent. The single source of truth is
    // `compute_cascade`'s internal walk.
    let layer1_config = toml::Table::new();
    let layer1_scope = toml::Table::new();
    let layer1_severity = mockspace_core::lint::GateSeverity::uniform(
        mockspace_core::lint::Severity::Warn,
    );

    let CascadeOutput {
        merged_config,
        merged_scope,
        resolved_chain: _,
        resolved_severity,
    } = compute_cascade(CascadeInputs {
        lint_name,
        layer1_config,
        layer1_scope,
        layer1_severity,
        user_block: Some(user_block),
        workspace_defaults,
        overrides,
        preset_source,
    })?;

    let only_staged = extract_only_staged(lint_name, descriptor.staging_aware, Some(user_block))?;

    let scope_config: crate::config_types::ScopeConfig = toml::Value::Table(merged_scope.clone())
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError {
            lint_name: lint_name.to_string(),
            field_path: "scope".to_string(),
            kind: ConfigErrorKind::InvalidValue,
            message: format!("scope config parse failed: {e}"),
            source_location: None,
        })?;
    let scope_filter = crate::scope_filter::ScopeFilter::from_config(lint_name, &scope_config)?;

    // The primitive constructor expects `&'static str` for name and
    // description (carried into the resulting Lint impl's Finding
    // records). Synthesised names come from runtime TOML; leak them
    // into static storage. Bounded by the user's lints.toml at engine
    // startup; lives for the whole engine run; deliberate trade.
    let static_name: &'static str = Box::leak(lint_name.to_string().into_boxed_str());
    let description_owned = anchor
        .description
        .clone()
        .unwrap_or_else(|| format!("preset-resolved lint via `{}`", extends_ref.host));
    let static_description: &'static str = Box::leak(description_owned.into_boxed_str());

    let severity_for_instantiate = resolved_severity.unwrap_or(layer1_severity);

    let lint = (descriptor.instantiate)(
        static_name,
        static_description,
        severity_for_instantiate,
        &merged_config,
        &merged_scope,
    )?;

    Ok(InstantiatedLint {
        lint,
        mode: descriptor.mode,
        staging_aware: descriptor.staging_aware,
        editor_skip: descriptor.editor_skip,
        only_staged,
        scope_filter,
        resolved_severity,
    })
}

fn instantiate_with_cascade(
    entry: &CatalogEntry,
    user_lints: &HashMap<String, toml::Table>,
    workspace_defaults: &toml::Table,
    overrides: &OverrideCascade,
    preset_source: &dyn PresetSource,
) -> Result<InstantiatedLint, ConfigError> {
    // Parse catalog-default tables from their `&'static str` form.
    let layer1_config: toml::Table =
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
    let layer1_scope: toml::Table =
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

    let user_block = user_lints.get(entry.name);

    // finding_kinds validation runs against the entry-declared set
    // before the cascade computes (an unknown gate.finding_kinds key
    // is a contract violation regardless of what overlays would do).
    if let Some(block) = user_block {
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

    // Cascade math (Layers 1 through 5) lives in `compute_cascade`.
    // The output carries merged tables, the resolved preset chain,
    // and the resolved severity.
    let CascadeOutput {
        merged_config,
        merged_scope,
        resolved_chain: _resolved_chain,
        resolved_severity,
    } = compute_cascade(CascadeInputs {
        lint_name: entry.name,
        layer1_config,
        layer1_scope,
        layer1_severity: entry.default_severity,
        user_block,
        workspace_defaults,
        overrides,
        preset_source,
    })?;

    // only_staged extraction validates staging_aware-consistency +
    // unknown-gate names. Independent of the cascade output.
    let only_staged = extract_only_staged(entry.name, entry.staging_aware, user_block)?;

    // Compile per-document scope filter from the merged_scope table.
    let scope_config: crate::config_types::ScopeConfig = toml::Value::Table(merged_scope.clone())
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError {
            lint_name: entry.name.to_string(),
            field_path: "scope".to_string(),
            kind: ConfigErrorKind::InvalidValue,
            message: format!("scope config parse failed: {e}"),
            source_location: None,
        })?;
    let scope_filter = crate::scope_filter::ScopeFilter::from_config(entry.name, &scope_config)?;

    // Construct the lint.
    let lint = (entry.instantiate)(&merged_config, &merged_scope)?;
    Ok(InstantiatedLint {
        lint,
        mode: entry.mode,
        staging_aware: entry.staging_aware,
        editor_skip: entry.editor_skip,
        only_staged,
        scope_filter,
        resolved_severity,
    })
}

// =========================================================================
// TOML merge.
// =========================================================================

/// Shallow overlay: keys in `src` overwrite keys in `dst` at the top
/// level. Nested tables get recursive overlay. List-field merge ops
/// (`<field>.add = [...]` / `<field>.remove = [...]`) compose against
/// the array `dst` already carries; see [`is_list_merge_op`] for the
/// trigger shape and [`apply_list_merge`] for the merge order.
fn overlay(dst: &mut toml::Table, src: &toml::Table) {
    for (k, v) in src {
        if let toml::Value::Table(src_tbl) = v {
            if is_list_merge_op(src_tbl) {
                apply_list_merge_into(dst, k, src_tbl);
                continue;
            }
        }
        match (dst.get_mut(k), v) {
            (Some(toml::Value::Table(d)), toml::Value::Table(s)) => overlay(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Detect the list-merge operator shape: a non-empty table whose every
/// key is `add` or `remove` and whose every value is an array. Matches
/// the toml source `<field>.add = [...]` / `<field>.remove = [...]`
/// which parses into a sub-table on the field. Anything else (including
/// a table that carries `add` plus an unrelated key) falls through to
/// the normal table-merge path so nested-config sub-tables that happen
/// to contain an `add` field are not misidentified.
fn is_list_merge_op(t: &toml::Table) -> bool {
    !t.is_empty()
        && t.iter().all(|(k, v)| {
            (k == "add" || k == "remove") && matches!(v, toml::Value::Array(_))
        })
}

/// Apply a list-merge op rooted at `key` in `dst`. If `dst[key]` is an
/// array, mutate it in place; if missing, synthesize from `add`; if
/// present but not an array, fall back to replace (the user mixed list
/// ops with a scalar value, surfaced as a replace so the writer sees
/// the type mismatch through `cargo mock explain`).
fn apply_list_merge_into(dst: &mut toml::Table, key: &str, op: &toml::Table) {
    match dst.get_mut(key) {
        Some(toml::Value::Array(arr)) => apply_list_merge(arr, op),
        Some(_) => {
            // Type mismatch (scalar where array expected): replace with
            // whatever `add` carries so the writer sees the cascade
            // result, or remove the key entirely if only `remove` was
            // present.
            if let Some(toml::Value::Array(add)) = op.get("add") {
                dst.insert(key.to_string(), toml::Value::Array(add.clone()));
            } else {
                dst.remove(key);
            }
        }
        None => {
            if let Some(toml::Value::Array(add)) = op.get("add") {
                dst.insert(key.to_string(), toml::Value::Array(add.clone()));
            }
            // `remove` against a missing key is a no-op.
        }
    }
}

/// Merge order: `remove` first, then `add`. Removal is value-equality
/// based (TOML's `PartialEq` on `Value`). `add` is set-like: items
/// already present in `dst` are not duplicated. This gives consumers
/// intuitive semantics for token lists (`forbidden = [...]` and similar)
/// where double-adding a value should not multiply it in the output.
fn apply_list_merge(dst: &mut Vec<toml::Value>, op: &toml::Table) {
    if let Some(toml::Value::Array(remove)) = op.get("remove") {
        dst.retain(|item| !remove.contains(item));
    }
    if let Some(toml::Value::Array(add)) = op.get("add") {
        for item in add {
            if !dst.contains(item) {
                dst.push(item.clone());
            }
        }
    }
}

// =========================================================================
// Severity cascade (#566).
// =========================================================================

/// Convert a `mockspace_config::Severity` (4-variant TOML wire form) to
/// the engine's `mockspace_core::lint::Severity` (6-variant runtime
/// enum). The wire form is the consumer surface: `error` / `warn` /
/// `info` / `off`. The engine's enum carries extra variants (`Skip`,
/// `Hint`) that the wire form does not address; they remain reachable
/// to lint authors through `default_severity` but are not selectable
/// through the cascade overlay surface.
fn convert_severity(s: mockspace_config::Severity) -> mockspace_core::lint::Severity {
    use mockspace_core::lint::Severity as Core;
    use mockspace_config::Severity as Wire;
    match s {
        Wire::Error => Core::Error,
        Wire::Warn => Core::Warn,
        Wire::Info => Core::Info,
        Wire::Off => Core::Off,
    }
}

/// Overlay `gs` onto `dst`, applying each `Some` field and leaving
/// `None` fields pass-through. Mirrors the cascade memo's "gate
/// severities compose per-axis" rule: a preset can refine the
/// commit gate's severity without touching build or push.
fn overlay_gate_severities(
    dst: &mut mockspace_core::lint::GateSeverity,
    gs: &mockspace_config::GateSeverities,
) {
    if let Some(s) = gs.commit {
        dst.commit = convert_severity(s);
    }
    if let Some(s) = gs.build {
        dst.build = convert_severity(s);
    }
    if let Some(s) = gs.push {
        dst.push = convert_severity(s);
    }
}

/// Walk the resolved preset chain + user TOML + CLI overrides building
/// the cascaded `GateSeverity`. Returns `None` if no cascade layer
/// touches severity at any axis, so the engine falls back to the
/// lint's `default_severity()` per the documented invariant.
///
/// `default_severity` is the catalog's per-gate baseline (the same
/// value `entry.default_severity` carries). It seeds `acc` so untouched
/// axes inherit the catalog default; a consumer setting only `commit`
/// does not silently downgrade `build` / `push` to Off. This matches
/// the cascade memo's `cargo mock explain` output, which renders each
/// axis with its own per-layer origin annotation rather than the
/// touched-axis-wins-takes-all path.
///
/// Cascade order (low to high precedence):
///   1. Preset chain, innermost-first. The starting preset (consumer's
///      direct reference) overlays whatever deeper extends-targets
///      established. Each preset's `[severity]` block can refine one
///      or more of commit / build / push independently.
///   2. User TOML per-lint `[lints.<name>]` `commit` / `build` / `push`
///      fields. Wins over preset chain.
///   3. CLI `--severity-override <name>=<sev>` (uniform across all
///      gates; bumps the same severity onto every gate of the named
///      lint).
fn resolve_severity_cascade(
    default_severity: mockspace_core::lint::GateSeverity,
    chain: &[PresetFile],
    user_block: Option<&toml::Table>,
    cli_overrides: &std::collections::HashMap<String, mockspace_core::lint::Severity>,
    lint_name: &str,
) -> Option<mockspace_core::lint::GateSeverity> {
    use mockspace_core::lint::GateSeverity;

    let mut touched = false;
    // Seed from the catalog default so untouched axes inherit the
    // lint's per-gate default rather than the permissive Off baseline.
    // The returned `Option` distinguishes "no layer touched severity"
    // (None, engine still uses lint default identically) from "layers
    // touched it" (Some, engine uses the cascaded value, where
    // untouched axes still equal the catalog default).
    let mut acc = default_severity;

    for preset in chain {
        let gs = &preset.severity;
        if gs.commit.is_some() || gs.build.is_some() || gs.push.is_some() {
            touched = true;
            overlay_gate_severities(&mut acc, gs);
        }
    }

    if let Some(block) = user_block {
        let user_gs = mockspace_config::GateSeverities {
            commit: block.get("commit").and_then(parse_wire_severity),
            build: block.get("build").and_then(parse_wire_severity),
            push: block.get("push").and_then(parse_wire_severity),
        };
        if user_gs.commit.is_some() || user_gs.build.is_some() || user_gs.push.is_some() {
            touched = true;
            overlay_gate_severities(&mut acc, &user_gs);
        }
    }

    if let Some(cli_sev) = cli_overrides.get(lint_name) {
        touched = true;
        acc = GateSeverity::uniform(*cli_sev);
    }

    if touched { Some(acc) } else { None }
}

/// Parse a `toml::Value` representing a severity string (one of
/// `"error"` / `"warn"` / `"info"` / `"off"`) into the wire-form enum.
/// Returns `None` on type mismatch or unknown variant; the cascade
/// then treats the field as absent.
fn parse_wire_severity(v: &toml::Value) -> Option<mockspace_config::Severity> {
    use mockspace_config::Severity as Wire;
    match v.as_str()? {
        "error" => Some(Wire::Error),
        "warn" => Some(Wire::Warn),
        "info" => Some(Wire::Info),
        "off" => Some(Wire::Off),
        _ => None,
    }
}

// =========================================================================
// Preset chain application.
// =========================================================================

/// Convert a [`mockspace_config::PresetResolveError`] into the engine's
/// [`ConfigError`] channel. Preset resolution failures are config faults
/// in the lints.toml that named the bad extends chain, so this folds
/// them into the same per-lint reporting surface that other cascade
/// faults go through.
fn preset_error_to_config_error(
    lint_name: &str,
    field_path: &str,
    err: PresetResolveError,
) -> ConfigError {
    ConfigError {
        lint_name: lint_name.to_string(),
        field_path: field_path.to_string(),
        kind: ConfigErrorKind::InvalidValue,
        message: format!("{err}"),
        source_location: None,
    }
}

/// Apply the resolved preset chain over `merged_config` and `merged_scope`,
/// in innermost-first order. The resolver already returned the chain
/// with the deepest extends-target at index 0 and the starting preset
/// at the tail (per `resolve_preset_chain`'s contract). Walking in that
/// same order means each layer overlays whatever the deeper layer had
/// already established, so the starting preset (the consumer's direct
/// reference) wins where it touches a field and the deeper presets show
/// through only where the starting preset is silent.
///
/// Config and scope are independent overlays: the preset's `config`
/// BTreeMap targets `merged_config`; the preset's `scope` BTreeMap
/// targets `merged_scope`. Severity overlay is not applied here; see
/// `instantiate_with_cascade`'s level-2 doc comment for the gap and
/// task #566 for the planned wiring.
fn apply_preset_chain(
    chain: &[PresetFile],
    merged_config: &mut toml::Table,
    merged_scope: &mut toml::Table,
) {
    for preset in chain {
        if !preset.config.is_empty() {
            let overlay_table: toml::Table =
                preset.config.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            overlay(merged_config, &overlay_table);
        }
        if !preset.scope.is_empty() {
            let overlay_table: toml::Table =
                preset.scope.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            overlay(merged_scope, &overlay_table);
        }
    }
}

// =========================================================================
// File discovery.
// =========================================================================

/// Discover and parse the consumer's lints / mockspace TOML file for
/// `workspace_root`. Searches in canonical order: `lints.toml`,
/// `mock/lints.toml`, `mockspace.toml`; returns the first match parsed
/// as a [`LintsTomlFile`]. Returns the [`LintsTomlFile::default()`]
/// (empty maps) when no file is found, so call sites can use the
/// same shape whether or not the consumer authored one.
///
/// Public for the CLI `cargo mock explain` subcommand and any other
/// caller that needs the parsed-but-uncascaded TOML state without
/// going through the full [`LintsConfig::load`] pipeline. Internal
/// callers in [`LintsConfig::load`] continue to use this helper too.
pub fn find_and_read_lints_toml(workspace_root: &Path) -> Result<LintsTomlFile, LoadError> {
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

    // ---- issue #106 bisection probes ------------------------------------
    //
    // The two tests below isolate where the cascade-vs-dispatch gap
    // lives for `[defaults] visibility = "any"`. Each test makes one
    // claim against one layer of the resolved-config pipeline; their
    // pass/fail pattern bisects between:
    //
    //   * overlay() not merging the [defaults] override into
    //     merged_config (Layer 3 gap), OR
    //   * AstTypePositionConfig deserialization dropping the value
    //     (deserializer gap), OR
    //   * the gap is downstream of both (engine.run / scope filter /
    //     lint enabling).
    //
    // Result: both probes PASS. The gap is downstream of overlay() and
    // deserialization. The original repro used `visibility = "all"`,
    // which is not a Visibility variant (only Any | Public exist); the
    // resulting ConfigError gets silently swallowed in the load path.
    // Task #572 surfaces that error as a visible diagnostic.

    #[test]
    fn issue_106_overlay_propagates_visibility_into_merged_table() {
        // The catalog default for ast-type-position carries
        // `visibility = "public"`. A [defaults] block setting
        // `visibility = "any"` should overwrite that key after
        // overlay() runs.
        let mut merged: toml::Table = "forbidden_types = [\"usize\"]\n\
             positions = [\"fn-return\"]\n\
             visibility = \"public\"\n\
             replacements = []\n"
            .parse()
            .unwrap();
        let workspace_defaults: toml::Table = "visibility = \"any\"\n".parse().unwrap();
        overlay(&mut merged, &workspace_defaults);
        assert_eq!(
            merged.get("visibility").and_then(|v| v.as_str()),
            Some("any"),
            "overlay() should leave merged_config with visibility=\"any\" \
             after applying [defaults] visibility = \"any\""
        );
    }

    #[test]
    fn issue_106_deserializer_reads_visibility_from_merged_table() {
        // Given a merged_config table with visibility = "any", the
        // AstTypePositionConfig deserializer at
        // ast_type_position.rs:316 should populate
        // parsed.visibility = Visibility::Any. This is the
        // second-leg bisection probe; combined with the overlay
        // probe above, the pair pinpoints which layer drops the
        // override.
        use crate::builtins::ast_type_position::AstTypePositionConfig;
        use crate::config_types::Visibility;
        let merged: toml::Table = "forbidden_types = [\"usize\"]\n\
             positions = [\"fn-return\"]\n\
             visibility = \"any\"\n\
             replacements = []\n"
            .parse()
            .unwrap();
        let parsed: AstTypePositionConfig =
            merged.try_into().expect("deserialize must succeed");
        assert_eq!(
            parsed.visibility,
            Visibility::Any,
            "deserialized AstTypePositionConfig should reflect \
             visibility=\"any\" from the merged_config table"
        );
    }

    // ---- list-merge .add / .remove (#567) -------------------------------

    fn arr_strs(t: &toml::Table, key: &str) -> Vec<String> {
        t.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn list_merge_add_appends_to_existing_array() {
        let mut dst: toml::Table = r#"forbidden = ["alloc", "std"]"#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { add = ["dyn"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["alloc", "std", "dyn"]);
    }

    #[test]
    fn list_merge_remove_drops_named_items() {
        let mut dst: toml::Table = r#"forbidden = ["alloc", "std", "core"]"#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { remove = ["std"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["alloc", "core"]);
    }

    #[test]
    fn list_merge_remove_runs_before_add() {
        // Same item removed and added: net add (remove first, then add).
        let mut dst: toml::Table = r#"forbidden = ["alloc"]"#.parse().unwrap();
        let src: toml::Table =
            r#"forbidden = { remove = ["alloc"], add = ["alloc", "std"] }"#
                .parse()
                .unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["alloc", "std"]);
    }

    #[test]
    fn list_merge_add_dedupes_items_already_present() {
        let mut dst: toml::Table = r#"forbidden = ["alloc"]"#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { add = ["alloc", "std"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["alloc", "std"]);
    }

    #[test]
    fn list_merge_creates_field_from_add_when_dst_missing() {
        let mut dst: toml::Table = "".parse().unwrap();
        let src: toml::Table = r#"forbidden = { add = ["alloc"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["alloc"]);
    }

    #[test]
    fn list_merge_remove_against_missing_field_is_noop() {
        let mut dst: toml::Table = "".parse().unwrap();
        let src: toml::Table = r#"forbidden = { remove = ["alloc"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert!(dst.get("forbidden").is_none());
    }

    #[test]
    fn list_merge_op_with_unrelated_key_falls_through_to_table_merge() {
        // `extra` key alongside `add` disqualifies this as a list-merge op;
        // the overlay match's default arm runs and inserts the whole src
        // table over dst[forbidden] (array-to-table type mismatch hits
        // the same `_` arm a scalar replace would).
        let mut dst: toml::Table = r#"forbidden = ["alloc"]"#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { add = ["dyn"], extra = "x" }"#
            .parse()
            .unwrap();
        overlay(&mut dst, &src);
        // dst[forbidden] becomes a clone of the src table; the list-merge
        // op was rejected so the table replaces the array verbatim.
        let result = dst.get("forbidden").unwrap();
        assert!(result.is_table(), "expected table after fall-through, got {result:?}");
    }

    #[test]
    fn list_merge_chain_remove_followed_by_add_at_outer_layer() {
        // Simulates a three-layer cascade: deepest layer establishes the
        // base list, middle layer adds an item, outer layer removes the
        // base. Final state matches the documented chain semantics.
        let mut merged: toml::Table = r#"forbidden = ["alloc"]"#.parse().unwrap();
        // Middle layer: add "bar".
        let middle: toml::Table = r#"forbidden = { add = ["bar"] }"#.parse().unwrap();
        overlay(&mut merged, &middle);
        assert_eq!(arr_strs(&merged, "forbidden"), vec!["alloc", "bar"]);
        // Outer layer: remove "alloc".
        let outer: toml::Table = r#"forbidden = { remove = ["alloc"] }"#.parse().unwrap();
        overlay(&mut merged, &outer);
        assert_eq!(arr_strs(&merged, "forbidden"), vec!["bar"]);
    }

    #[test]
    fn list_merge_op_with_scalar_dst_replaces_with_add_array() {
        // Type mismatch (scalar where the merge op expected array): the
        // op falls back to a replace using `add` as the new value. The
        // mismatch surfaces in `cargo mock explain` cascade output.
        let mut dst: toml::Table = r#"forbidden = "alloc""#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { add = ["dyn"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert_eq!(arr_strs(&dst, "forbidden"), vec!["dyn"]);
    }

    #[test]
    fn list_merge_op_with_scalar_dst_and_remove_only_deletes_key() {
        // Type mismatch (scalar dst) plus a `remove`-only op: the key
        // is deleted entirely, because there is no `add` value to
        // synthesise a replacement from. Pins the silent-delete
        // branch in `apply_list_merge_into` documented at the
        // function's "Some(_)" arm.
        let mut dst: toml::Table = r#"forbidden = "alloc""#.parse().unwrap();
        let src: toml::Table = r#"forbidden = { remove = ["alloc"] }"#.parse().unwrap();
        overlay(&mut dst, &src);
        assert!(dst.get("forbidden").is_none());
    }

    // ---- severity cascade (#566) ----------------------------------------

    use mockspace_config::{GateSeverities, Severity as WireSeverity};

    fn preset_with_severity(commit: Option<WireSeverity>, build: Option<WireSeverity>, push: Option<WireSeverity>) -> PresetFile {
        PresetFile {
            schema_version: "1.0".to_string(),
            name: "test".to_string(),
            primitive: "token-scan".to_string(),
            description: None,
            extends: None,
            config: Default::default(),
            severity: GateSeverities { commit, build, push },
            scope: Default::default(),
        }
    }

    /// Lint catalog default used across the severity-cascade tests:
    /// uniform Warn at every gate. Used so the "untouched axes
    /// inherit catalog default" property is observable.
    const DEFAULT_WARN: mockspace_core::lint::GateSeverity =
        mockspace_core::lint::GateSeverity::uniform(mockspace_core::lint::Severity::Warn);

    #[test]
    fn severity_cascade_returns_none_when_nothing_touches_it() {
        let chain: Vec<PresetFile> = Vec::new();
        let cli = HashMap::new();
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, None, &cli, "x");
        assert!(resolved.is_none());
    }

    #[test]
    fn severity_cascade_applies_preset_to_specific_gate_keeping_default_elsewhere() {
        // Preset refines only `commit`. `build` and `push` inherit
        // the catalog default (uniform Warn), not the permissive Off
        // baseline. This is the load-bearing per-axis-inheritance
        // property that the cascade memo's `cargo mock explain`
        // output documents.
        let chain = vec![preset_with_severity(Some(WireSeverity::Error), None, None)];
        let cli = HashMap::new();
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, None, &cli, "x").unwrap();
        assert_eq!(resolved.commit, mockspace_core::lint::Severity::Error);
        assert_eq!(resolved.build, mockspace_core::lint::Severity::Warn);
        assert_eq!(resolved.push, mockspace_core::lint::Severity::Warn);
    }

    #[test]
    fn severity_cascade_starting_preset_overlays_deeper_extends_target() {
        // Innermost-first walk: chain[0] is the deepest extends-target
        // (resolved earlier in the chain). chain[1] is the starting
        // preset (consumer's direct reference). Deep sets commit=Warn,
        // starting overrides with commit=Error.
        let chain = vec![
            preset_with_severity(Some(WireSeverity::Warn), None, None),
            preset_with_severity(Some(WireSeverity::Error), None, None),
        ];
        let cli = HashMap::new();
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, None, &cli, "x").unwrap();
        assert_eq!(resolved.commit, mockspace_core::lint::Severity::Error);
    }

    #[test]
    fn severity_cascade_user_toml_wins_over_preset_chain() {
        let chain = vec![preset_with_severity(Some(WireSeverity::Warn), None, None)];
        let mut user_block = toml::Table::new();
        user_block.insert("commit".to_string(), toml::Value::String("error".to_string()));
        let cli = HashMap::new();
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, Some(&user_block), &cli, "x").unwrap();
        assert_eq!(resolved.commit, mockspace_core::lint::Severity::Error);
    }

    #[test]
    fn severity_cascade_cli_override_wins_and_is_uniform() {
        let chain = vec![preset_with_severity(Some(WireSeverity::Warn), Some(WireSeverity::Info), Some(WireSeverity::Info))];
        let mut cli = HashMap::new();
        cli.insert("x".to_string(), mockspace_core::lint::Severity::Error);
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, None, &cli, "x").unwrap();
        // CLI override is uniform across all gates per OverrideCascade
        // documentation; every gate becomes Error.
        assert_eq!(resolved.commit, mockspace_core::lint::Severity::Error);
        assert_eq!(resolved.build, mockspace_core::lint::Severity::Error);
        assert_eq!(resolved.push, mockspace_core::lint::Severity::Error);
    }

    #[test]
    fn severity_cascade_per_axis_inherits_default_for_untouched_axes() {
        // Preset sets commit; user TOML sets build; push remains at
        // the catalog default (uniform Warn). This is the corrected
        // per-axis behaviour: untouched axes inherit the catalog
        // default rather than silently downgrade to Off.
        let chain = vec![preset_with_severity(Some(WireSeverity::Error), None, None)];
        let mut user_block = toml::Table::new();
        user_block.insert("build".to_string(), toml::Value::String("info".to_string()));
        let cli = HashMap::new();
        let resolved = resolve_severity_cascade(DEFAULT_WARN, &chain, Some(&user_block), &cli, "x").unwrap();
        assert_eq!(resolved.commit, mockspace_core::lint::Severity::Error);
        assert_eq!(resolved.build, mockspace_core::lint::Severity::Info);
        assert_eq!(resolved.push, mockspace_core::lint::Severity::Warn);
    }

    #[test]
    fn severity_cascade_unknown_string_in_user_toml_falls_through() {
        // Garbage severity string in user TOML: parse_wire_severity
        // returns None, and that axis is treated as not-set. The
        // cascade returns None if no other layer set anything.
        let chain: Vec<PresetFile> = Vec::new();
        let mut user_block = toml::Table::new();
        user_block.insert("commit".to_string(), toml::Value::String("BANANA".to_string()));
        let cli = HashMap::new();
        assert!(resolve_severity_cascade(DEFAULT_WARN, &chain, Some(&user_block), &cli, "x").is_none());
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

    // ---- namespace-conflict detection (slice 5 of lint:prop) ------------

    use crate::lint::Lint;
    use mockspace_core::lint::{GateSeverity, Severity};

    /// Build an `InstantiatedLint` carrying a stub `Lint` whose only
    /// non-default method is `declared_props`. The catalog metadata
    /// fields (mode, staging_aware, etc.) are not exercised by the
    /// detector and use harmless defaults.
    fn stub_entry(name: &'static str, props: &'static [&'static str]) -> InstantiatedLint {
        struct StubLint {
            name: &'static str,
            props: &'static [&'static str],
        }
        impl Lint for StubLint {
            fn name(&self) -> &'static str {
                self.name
            }
            fn description(&self) -> &'static str {
                "stub"
            }
            fn default_severity(&self) -> GateSeverity {
                GateSeverity::uniform(Severity::Warn)
            }
            fn declared_props(&self) -> &'static [&'static str] {
                self.props
            }
        }
        InstantiatedLint {
            lint: Box::new(StubLint { name, props }),
            mode: crate::lint::LintMode::PerDocument,
            staging_aware: false,
            editor_skip: false,
            only_staged: OnlyStaged::default(),
            scope_filter: crate::scope_filter::ScopeFilter::from_config(
                name,
                &crate::config_types::ScopeConfig::default(),
            )
            .expect("default scope config compiles"),
            resolved_severity: None,
        }
    }

    #[test]
    fn detector_emits_nothing_when_no_props_declared() {
        let entries = vec![stub_entry("a", &[]), stub_entry("b", &[])];
        assert!(detect_prop_name_conflicts(&entries).is_empty());
    }

    #[test]
    fn detector_silent_for_single_declaring_lint() {
        let entries = vec![
            stub_entry("a", &["audited"]),
            stub_entry("b", &["other_prop"]),
        ];
        assert!(detect_prop_name_conflicts(&entries).is_empty());
    }

    #[test]
    fn detector_warns_on_unqualified_collision() {
        let entries = vec![
            stub_entry("lint-a", &["audited"]),
            stub_entry("lint-b", &["audited"]),
        ];
        let warnings = detect_prop_name_conflicts(&entries);
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            StartupWarning::PropNameConflict { prop_name, lints } => {
                assert_eq!(prop_name, "audited");
                assert_eq!(lints, &vec!["lint-a".to_string(), "lint-b".to_string()]);
            }
        }
    }

    #[test]
    fn detector_silent_for_first_party_namespaced_collision() {
        let entries = vec![
            stub_entry("lint-a", &["mockspace::audited"]),
            stub_entry("lint-b", &["mockspace::audited"]),
        ];
        // mockspace:: is the reserved first-party namespace; collisions
        // among first-party prop names are silent (same pack).
        assert!(detect_prop_name_conflicts(&entries).is_empty());
    }

    #[test]
    fn detector_handles_three_way_conflict_as_one_warning() {
        let entries = vec![
            stub_entry("lint-a", &["arena_size"]),
            stub_entry("lint-b", &["arena_size"]),
            stub_entry("lint-c", &["arena_size"]),
        ];
        let warnings = detect_prop_name_conflicts(&entries);
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            StartupWarning::PropNameConflict { prop_name, lints } => {
                assert_eq!(prop_name, "arena_size");
                assert_eq!(
                    lints,
                    &vec![
                        "lint-a".to_string(),
                        "lint-b".to_string(),
                        "lint-c".to_string()
                    ]
                );
            }
        }
    }

    #[test]
    fn detector_deduplicates_repeated_self_declarations() {
        // A single lint listing the same prop name twice does not
        // self-conflict; the detector dedupes before checking the
        // multi-lint threshold.
        let entries = vec![stub_entry("only-lint", &["audited", "audited"])];
        assert!(detect_prop_name_conflicts(&entries).is_empty());
    }

    #[test]
    fn detector_emits_warning_per_distinct_conflicting_prop() {
        let entries = vec![
            stub_entry("lint-a", &["audited", "arena_size"]),
            stub_entry("lint-b", &["audited", "arena_size"]),
        ];
        let warnings = detect_prop_name_conflicts(&entries);
        assert_eq!(warnings.len(), 2);
        let mut names: Vec<&str> = warnings
            .iter()
            .map(|w| match w {
                StartupWarning::PropNameConflict { prop_name, .. } => prop_name.as_str(),
            })
            .collect();
        names.sort();
        assert_eq!(names, vec!["arena_size", "audited"]);
    }

    #[test]
    fn detector_mixed_collision_warns_only_on_unqualified_axis() {
        // One unqualified prop name collides; one first-party-namespaced
        // prop name collides. Only the unqualified one warns.
        let entries = vec![
            stub_entry("lint-a", &["audited", "mockspace::reviewed"]),
            stub_entry("lint-b", &["audited", "mockspace::reviewed"]),
        ];
        let warnings = detect_prop_name_conflicts(&entries);
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            StartupWarning::PropNameConflict { prop_name, .. } => {
                assert_eq!(prop_name, "audited");
            }
        }
    }

    #[test]
    fn lints_config_populates_startup_warnings_from_catalog_defaults() {
        // The registered catalog presently has no prop-declaring lints,
        // so the warnings field is empty. The shape of the call must
        // still expose the field.
        let cfg = LintsConfig::from_catalog_defaults();
        assert!(cfg.startup_warnings.is_empty());
    }

    #[test]
    fn load_reads_lints_toml() {
        // A user TOML block for a lint name that is not in the
        // registered catalog and carries no `extends` shorthand emits
        // a config error (per the #611 contract: the v2 engine
        // surfaces the typo / missing reference rather than silently
        // ignoring the block).
        let tmp = std::env::temp_dir().join("mockspace_test_lints_load_real");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("lints.toml"),
            "[lints.no-such-lint.config]\ntokens = []\n",
        )
        .unwrap();
        let cfg = LintsConfig::load(&tmp, OverrideCascade::default()).unwrap();
        let found = cfg
            .config_errors
            .iter()
            .find(|e| e.lint_name == "no-such-lint");
        assert!(
            found.is_some(),
            "expected a config error naming the missing extends reference"
        );
        let err = found.unwrap();
        assert_eq!(err.field_path, "extends");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- preset cascade integration (#539) ---------------------------------

    use mockspace_config::{PresetFile, PresetRef, PresetResolveError, PresetSource};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        /// Most-recent `(config, scope)` pair the catalog stub's
        /// `instantiate` saw. Each test invocation populates this from
        /// its own thread (cargo test runs each `#[test]` on a worker
        /// thread, and thread-locals isolate per-worker), then drains
        /// it via `extract_captured` before the test returns. Using a
        /// `RefCell<Option<...>>` keeps the surface `unsafe`-free; the
        /// previous trait-object-to-concrete pointer-cast approach was
        /// not soundly licensed.
        static CAPTURED: RefCell<Option<(toml::Table, toml::Table)>> =
            const { RefCell::new(None) };
    }

    /// Minimal `Lint` stub. The constructor (`captured_instantiate`)
    /// writes the merged config/scope it sees into `CAPTURED`; the test
    /// drains the thread-local via `extract_captured` to assert what the
    /// cascade produced. The lint's own behaviour is intentionally inert.
    struct NoOpLint;

    impl Lint for NoOpLint {
        fn name(&self) -> &'static str {
            "captured"
        }
        fn description(&self) -> &'static str {
            "test stub; merged tables routed to CAPTURED"
        }
        fn default_severity(&self) -> GateSeverity {
            GateSeverity::uniform(Severity::Warn)
        }
    }

    fn captured_instantiate(
        config: &toml::Table,
        scope: &toml::Table,
    ) -> Result<Box<dyn Lint>, ConfigError> {
        CAPTURED.with(|c| {
            *c.borrow_mut() = Some((config.clone(), scope.clone()));
        });
        Ok(Box::new(NoOpLint))
    }

    fn captured_entry() -> CatalogEntry {
        CatalogEntry {
            name: "captured",
            description: "test stub",
            kind: "test-captured",
            // Default config seeds two fields the preset will overwrite + leave
            // alone so the assertions can distinguish overlay from replace.
            default_config: "level = \"catalog\"\nuntouched = true\n",
            default_scope: "category = \"catalog\"\n",
            default_severity: GateSeverity::uniform(Severity::Warn),
            default_impact: None,
            default_category: None,
            doc_url: None,
            mode: crate::lint::LintMode::PerDocument,
            staging_aware: false,
            editor_skip: false,
            instantiate: captured_instantiate,
            finding_kinds: &[],
        }
    }

    /// In-memory preset source keyed by `(host, name)`. Mirrors the test
    /// double in `mockspace_config::preset_resolver::tests::MockSource`
    /// but lives at the engine layer so the cascade tests can compose
    /// real-shaped presets without touching the embedded table.
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
                .ok_or_else(|| PresetResolveError::NotFound {
                    host: preset_ref.host.clone(),
                    name: preset_ref.name.clone(),
                })
        }
    }

    fn preset(name: &str, extends: Option<&str>, config: &str, scope: &str) -> PresetFile {
        let config: BTreeMap<String, toml::Value> = config
            .parse::<toml::Table>()
            .unwrap()
            .into_iter()
            .collect();
        let scope: BTreeMap<String, toml::Value> =
            scope.parse::<toml::Table>().unwrap().into_iter().collect();
        PresetFile {
            schema_version: "1.0".to_string(),
            name: name.to_string(),
            primitive: "test-captured".to_string(),
            description: None,
            extends: extends.map(String::from),
            config,
            severity: Default::default(),
            scope,
        }
    }

    /// Drain the thread-local set by the most-recent
    /// `captured_instantiate` on this thread. Drops the lint box once
    /// the cascade has handed it back; the merged tables are already
    /// recorded in `CAPTURED`.
    fn extract_captured(_lint: Box<dyn Lint>) -> (toml::Table, toml::Table) {
        CAPTURED.with(|c| {
            c.borrow_mut()
                .take()
                .expect("captured_instantiate ran on this thread")
        })
    }

    #[test]
    fn preset_chain_overlays_config_between_catalog_and_workspace() {
        let entry = captured_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset(
                "base",
                None,
                "level = \"preset\"\nadded = 1\n",
                "category = \"preset\"\n",
            ),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::base\"\n".parse().unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        let result =
            instantiate_with_cascade(&entry, &user_lints, &workspace_defaults, &overrides, &source)
                .expect("cascade succeeds");
        let (config, scope) = extract_captured(result.lint);
        // Preset overlaid the catalog's `level = "catalog"`; the
        // catalog's `untouched = true` survived because the preset did
        // not name it.
        assert_eq!(config.get("level").unwrap().as_str(), Some("preset"));
        assert_eq!(config.get("untouched").unwrap().as_bool(), Some(true));
        assert_eq!(config.get("added").unwrap().as_integer(), Some(1));
        assert_eq!(scope.get("category").unwrap().as_str(), Some("preset"));
    }

    #[test]
    fn per_lint_config_wins_over_preset_overlay() {
        let entry = captured_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset("base", None, "level = \"preset\"\n", ""),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table =
            "extends = \"mockspace::base\"\n[config]\nlevel = \"user\"\n"
                .parse()
                .unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        let result =
            instantiate_with_cascade(&entry, &user_lints, &workspace_defaults, &overrides, &source)
                .expect("cascade succeeds");
        let (config, _scope) = extract_captured(result.lint);
        assert_eq!(config.get("level").unwrap().as_str(), Some("user"));
    }

    #[test]
    fn preset_chain_resolves_innermost_first_outer_wins_on_overlap() {
        let entry = captured_entry();
        let source = InMemorySource::new()
            .insert(
                "mockspace",
                preset("base", None, "level = \"base\"\nbase_field = true\n", ""),
            )
            .insert(
                "mockspace",
                preset(
                    "outer",
                    Some("mockspace::base"),
                    "level = \"outer\"\nouter_field = true\n",
                    "",
                ),
            );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::outer\"\n".parse().unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        let result =
            instantiate_with_cascade(&entry, &user_lints, &workspace_defaults, &overrides, &source)
                .expect("cascade succeeds");
        let (config, _scope) = extract_captured(result.lint);
        // Outer preset's `level` field wins over the inner preset's
        // (which would have been written first).
        assert_eq!(config.get("level").unwrap().as_str(), Some("outer"));
        // Both presets' unique fields survive.
        assert_eq!(config.get("base_field").unwrap().as_bool(), Some(true));
        assert_eq!(config.get("outer_field").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn missing_extends_target_surfaces_as_config_error() {
        let entry = captured_entry();
        let source = InMemorySource::new();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::absent\"\n".parse().unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        match instantiate_with_cascade(
            &entry,
            &user_lints,
            &workspace_defaults,
            &overrides,
            &source,
        ) {
            Err(ConfigError { kind, field_path, message, .. }) => {
                assert!(matches!(kind, ConfigErrorKind::InvalidValue));
                assert_eq!(field_path, "extends");
                assert!(message.contains("absent"), "diagnostic should name the missing preset; got `{message}`");
            }
            Ok(_) => panic!("expected ConfigError for missing extends target"),
        }
    }

    #[test]
    fn cycle_in_preset_chain_surfaces_as_config_error() {
        let entry = captured_entry();
        let source = InMemorySource::new().insert(
            "mockspace",
            preset("loopy", Some("mockspace::loopy"), "", ""),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::loopy\"\n".parse().unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        match instantiate_with_cascade(
            &entry,
            &user_lints,
            &workspace_defaults,
            &overrides,
            &source,
        ) {
            Err(ConfigError { kind, field_path, message, .. }) => {
                assert!(matches!(kind, ConfigErrorKind::InvalidValue));
                assert_eq!(field_path, "extends");
                assert!(message.contains("cycle"), "diagnostic should name the cycle; got `{message}`");
            }
            Ok(_) => panic!("expected ConfigError for cyclic extends chain"),
        }
    }

    #[test]
    fn non_string_extends_value_is_rejected() {
        let entry = captured_entry();
        let source = InMemorySource::new();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = 42\n".parse().unwrap();
        user_lints.insert("captured".to_string(), user_block);
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        match instantiate_with_cascade(
            &entry,
            &user_lints,
            &workspace_defaults,
            &overrides,
            &source,
        ) {
            Err(ConfigError { kind, field_path, .. }) => {
                assert!(matches!(kind, ConfigErrorKind::InvalidValue));
                assert_eq!(field_path, "extends");
            }
            Ok(_) => panic!("expected ConfigError for non-string extends"),
        }
    }

    #[test]
    fn absent_extends_leaves_cascade_untouched_by_preset_layer() {
        let entry = captured_entry();
        let source = InMemorySource::new(); // unused; presence proven by no resolve calls
        let user_lints: HashMap<String, toml::Table> = HashMap::new();
        let workspace_defaults = toml::Table::new();
        let overrides = OverrideCascade::default();
        let result =
            instantiate_with_cascade(&entry, &user_lints, &workspace_defaults, &overrides, &source)
                .expect("cascade succeeds without extends");
        let (config, scope) = extract_captured(result.lint);
        // Catalog defaults flow through unchanged.
        assert_eq!(config.get("level").unwrap().as_str(), Some("catalog"));
        assert_eq!(config.get("untouched").unwrap().as_bool(), Some(true));
        assert_eq!(scope.get("category").unwrap().as_str(), Some("catalog"));
    }

    // ---- preset-as-catalog synthesis path (#611 PR-3) -------------------

    /// Build a PresetFile with an explicit primitive name. Used for
    /// the synthesised path; the regular `preset(...)` helper hardcodes
    /// the test-captured primitive which is not in PRIMITIVE_DESCRIPTORS.
    fn preset_with_primitive(
        name: &str,
        primitive: &str,
        extends: Option<&str>,
        config_toml: &str,
    ) -> PresetFile {
        let config: BTreeMap<String, toml::Value> = config_toml
            .parse::<toml::Table>()
            .unwrap()
            .into_iter()
            .collect();
        PresetFile {
            schema_version: "1.0".to_string(),
            name: name.to_string(),
            primitive: primitive.to_string(),
            description: Some(format!("test preset {name}")),
            extends: extends.map(String::from),
            config,
            severity: Default::default(),
            scope: BTreeMap::new(),
        }
    }

    #[test]
    fn synthesise_extends_to_first_party_preset_produces_lint() {
        // A consumer references `mockspace::regex-probe` for a lint
        // name (`probe-lint`) that is NOT in the auto-registered
        // catalog. The synthesis path resolves the preset, looks up
        // `content-regex` in PRIMITIVE_DESCRIPTORS, and constructs an
        // InstantiatedLint. End state: `probe-lint` is in the
        // resulting entries set.
        let source = InMemorySource::new().insert(
            "mockspace",
            preset_with_primitive(
                "regex-probe",
                "content-regex",
                None,
                r#"
[[patterns]]
regex = "TODO"
message = "TODO marker"
finding_kind = "todo"
"#,
            ),
        );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::regex-probe\"\n".parse().unwrap();
        user_lints.insert("probe-lint".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints: user_lints,
            defaults: None,
        };
        let cfg = LintsConfig::from_inputs_with_source(
            user_toml,
            OverrideCascade::default(),
            &source,
        );
        assert!(
            cfg.config_errors.is_empty(),
            "expected no config errors, got: {:?}",
            cfg.config_errors
        );
        // The synthesised lint should be present in entries by name.
        // Catalog-registered PerDocument lints (no-bare-vec, no-manual-id,
        // etc.) would satisfy a count-only check; the name probe pins
        // that the synthesis path actually constructed `probe-lint`.
        let synthesised_present = cfg
            .entries
            .iter()
            .any(|e| e.lint.name() == "probe-lint");
        assert!(
            synthesised_present,
            "expected synthesised `probe-lint` in entries; got: {:?}",
            cfg.entries.iter().map(|e| e.lint.name()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unregistered_lint_without_extends_emits_config_error() {
        // [lints.zzz-no-such-thing] with no `extends` and no catalog
        // entry surfaces as ConfigError::InvalidValue with a message
        // naming the required field.
        let source = InMemorySource::new();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "[config]\nfoo = 1\n".parse().unwrap();
        user_lints.insert("zzz-no-such-thing".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints: user_lints,
            defaults: None,
        };
        let cfg = LintsConfig::from_inputs_with_source(
            user_toml,
            OverrideCascade::default(),
            &source,
        );
        let found = cfg
            .config_errors
            .iter()
            .find(|e| e.lint_name == "zzz-no-such-thing");
        assert!(
            found.is_some(),
            "expected a config error for unregistered lint without extends"
        );
        let err = found.unwrap();
        assert_eq!(err.field_path, "extends");
        assert!(matches!(err.kind, ConfigErrorKind::InvalidValue));
    }

    #[test]
    fn synthesise_extends_to_unknown_preset_emits_config_error() {
        let source = InMemorySource::new();
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::not-shipped\"\n".parse().unwrap();
        user_lints.insert("probe-lint".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints: user_lints,
            defaults: None,
        };
        let cfg = LintsConfig::from_inputs_with_source(
            user_toml,
            OverrideCascade::default(),
            &source,
        );
        let found = cfg
            .config_errors
            .iter()
            .find(|e| e.lint_name == "probe-lint");
        assert!(
            found.is_some(),
            "expected a config error for extends pointing at unknown preset"
        );
    }

    #[test]
    fn synthesise_chain_with_mismatched_primitive_emits_error() {
        // chain: probe-lint -> regex-probe -> token-probe, where
        // regex-probe.primitive = "content-regex" and
        // token-probe.primitive = "token-scan". Resolver should reject
        // the cross-primitive chain.
        let source = InMemorySource::new()
            .insert(
                "mockspace",
                preset_with_primitive(
                    "token-probe",
                    "token-scan",
                    None,
                    "tokens = [\"FOO\"]\n",
                ),
            )
            .insert(
                "mockspace",
                preset_with_primitive(
                    "regex-probe",
                    "content-regex",
                    Some("mockspace::token-probe"),
                    r#"
[[patterns]]
regex = "BAR"
message = "BAR marker"
finding_kind = "bar"
"#,
                ),
            );
        let mut user_lints: HashMap<String, toml::Table> = HashMap::new();
        let user_block: toml::Table = "extends = \"mockspace::regex-probe\"\n".parse().unwrap();
        user_lints.insert("probe-lint".to_string(), user_block);
        let user_toml = LintsTomlFile {
            lints: user_lints,
            defaults: None,
        };
        let cfg = LintsConfig::from_inputs_with_source(
            user_toml,
            OverrideCascade::default(),
            &source,
        );
        let found = cfg
            .config_errors
            .iter()
            .find(|e| e.lint_name == "probe-lint");
        assert!(
            found.is_some(),
            "expected a config error for mixed-primitive chain"
        );
        let err = found.unwrap();
        assert!(matches!(err.kind, ConfigErrorKind::ContradictsCatalog));
    }
}
