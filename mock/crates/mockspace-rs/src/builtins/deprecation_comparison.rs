//! `deprecation_comparison` bespoke primitive.
//!
//! Per schema design memo §4.17. Compares the symbol set listed in
//! deprecated CL files against the symbol set listed in active CL files
//! and reports drift: symbols present in deprecated CLs but absent from
//! active CLs (forgotten removals) or vice versa (orphan additions).
//!
//! Phase 2D-4 ships the contract surface; the actual CL-file parsing
//! plugs into the mockspace round directory walker once it's wired into
//! the project model. Until then the lint emits no findings; the empty
//! impl preserves the catalog slot for the migration plan.

use std::borrow::Cow;

use globset::Glob;
use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

use super::cross_doc_symbol::SymbolKind;

pub const KIND: &str = "deprecation-comparison";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct DeprecationComparisonConfig {
    pub active_cls_glob: String,
    pub deprecated_cls_glob: String,
    pub symbol_kinds: Vec<SymbolKind>,
}

pub struct DeprecationComparisonLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: DeprecationComparisonConfig,
    #[allow(dead_code)]
    active_glob: globset::GlobMatcher,
    #[allow(dead_code)]
    deprecated_glob: globset::GlobMatcher,
}

impl DeprecationComparisonLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: DeprecationComparisonConfig,
    ) -> Result<Self, ConfigError> {
        let active_glob = compile_glob(name, &config.active_cls_glob, "active_cls_glob")?;
        let deprecated_glob = compile_glob(name, &config.deprecated_cls_glob, "deprecated_cls_glob")?;
        Ok(Self {
            name,
            description,
            default_severity,
            config,
            active_glob,
            deprecated_glob,
        })
    }
}

fn compile_glob(
    lint_name: &'static str,
    pattern: &str,
    field: &str,
) -> Result<globset::GlobMatcher, ConfigError> {
    Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ConfigError {
            lint_name: lint_name.to_string(),
            field_path: field.to_string(),
            kind: ConfigErrorKind::UnparseableGlob {
                error: e.to_string(),
            },
            message: format!("glob `{pattern}` did not compile"),
            source_location: None,
        })
}

impl Lint for DeprecationComparisonLint {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn default_severity(&self) -> GateSeverity {
        self.default_severity
    }

    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        // Future hook: walk project documents matching `active_glob` and
        // `deprecated_glob`, parse symbol lists from each, emit findings
        // on set differences. Today the lint is a contract-surface stub;
        // CL parsing logic plugs in when the round walker is wired (Phase 2B).
        let _ = (ctx, project, sink, &self.config);
        Ok(())
    }
}

/// Emit a deprecation drift finding. Helper for future impl; kept here so
/// the message format is centralized.
#[allow(dead_code)]
fn emit_drift(
    lint_name: &'static str,
    symbol: &str,
    kind: SymbolKind,
    direction: &str,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    if severity.silent() {
        return;
    }
    sink.emit(Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: Some(Cow::Borrowed("drift")),
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(format!(
            "{direction}: {kind:?} `{symbol}` listed in deprecated CL but missing from active CL"
        )),
        span: Span::single_line("mock/design_rounds", 1, 1, 1),
        fix_suggestion: None,
        related_spans: Vec::new(),
        metadata: None,
    });
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: DeprecationComparisonConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("deprecation-comparison config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(DeprecationComparisonLint::new(
        name,
        description,
        default_severity,
        parsed,
    )?))
}
