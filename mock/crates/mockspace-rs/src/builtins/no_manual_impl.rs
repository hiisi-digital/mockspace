//! `no_manual_impl` bespoke primitive.
//!
//! Per schema design memo §4.14. Detects manual `impl` blocks of traits
//! that should be derived. Default forbidden set: Clone, Copy, Debug,
//! Default, PartialEq, Eq, Hash.

use std::borrow::Cow;
use std::collections::HashSet;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "no-manual-impl";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct NoManualImplConfig {
    pub forbidden_traits: Vec<String>,
}

impl Default for NoManualImplConfig {
    fn default() -> Self {
        Self {
            forbidden_traits: vec![
                "Clone".into(),
                "Copy".into(),
                "Debug".into(),
                "Default".into(),
                "PartialEq".into(),
                "Eq".into(),
                "Hash".into(),
            ],
        }
    }
}

pub struct NoManualImplLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    forbidden: HashSet<String>,
}

impl NoManualImplLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: NoManualImplConfig,
    ) -> Self {
        let forbidden = config.forbidden_traits.into_iter().collect();
        Self {
            name,
            description,
            default_severity,
            forbidden,
        }
    }
}

impl Lint for NoManualImplLint {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn default_severity(&self) -> GateSeverity {
        self.default_severity
    }
    fn needs_syn_ast(&self) -> bool {
        true
    }

    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let Some(file) = doc.ast() else {
            return Ok(());
        };
        let active = ctx.active_severity();
        for item in &file.items {
            if let syn::Item::Impl(impl_block) = item {
                if let Some((_, trait_path, _)) = &impl_block.trait_ {
                    if let Some(seg) = trait_path.segments.last() {
                        let trait_name = seg.ident.to_string();
                        if self.forbidden.contains(&trait_name) {
                            emit(self.name, doc.path(), &trait_name, active, sink);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn emit(
    lint_name: &'static str,
    path: &std::path::Path,
    trait_name: &str,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    if severity.silent() {
        return;
    }
    sink.emit(Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: None,
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(format!(
            "manual `impl {trait_name}` should be `#[derive({trait_name})]`"
        )),
        span: Span::single_line(path, 1, 1, trait_name.len() as u32),
        hint: None,
        help: None,
        suggestion: None,
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
    let parsed: NoManualImplConfig = if config.is_empty() {
        NoManualImplConfig::default()
    } else {
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("no-manual-impl config: {e}"),
                source_location: None,
            })?
    };
    Ok(Box::new(NoManualImplLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_types::Language;
    use crate::finding_sink::VecFindingSink;
    use mockspace_core::lint::{Gate, RunSurface};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    #[test]
    fn fires_on_manual_clone() {
        let lint = NoManualImplLint::new(
            "no-manual-impl",
            "",
            GateSeverity::uniform(Severity::Warn),
            NoManualImplConfig::default(),
        );
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "struct S; impl Clone for S { fn clone(&self) -> Self { S } }",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let cfg = EmptyCfg;
        let ctx = LintContext {
            gate: Gate::Commit,
            severities: GateSeverity::uniform(Severity::Warn),
            surface: RunSurface::Local,
            project_root: &root,
            config: &cfg,
        };
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
    }
}
