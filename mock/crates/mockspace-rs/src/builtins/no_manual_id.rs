//! `no_manual_id` bespoke primitive.
//!
//! Per schema design memo §4.13. Detects newtype patterns and type aliases
//! where the inner type is a primitive integer. Backs the workspace rule
//! "use semantic aliases (NodeId, SlotId) over hand-rolled wrappers".

use std::borrow::Cow;
use std::collections::HashSet;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "no-manual-id";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct NoManualIdConfig {
    pub primitive_inner_types: Vec<String>,
    #[serde(default = "default_true")]
    pub check_aliases: bool,
}

fn default_true() -> bool {
    true
}

pub struct NoManualIdLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: NoManualIdConfig,
    primitives: HashSet<String>,
}

impl NoManualIdLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: NoManualIdConfig,
    ) -> Self {
        let primitives = config.primitive_inner_types.iter().cloned().collect();
        Self {
            name,
            description,
            default_severity,
            config,
            primitives,
        }
    }

    fn check_inner(&self, name: &str, ty: &syn::Type) -> Option<String> {
        if let syn::Type::Path(tp) = ty {
            if let Some(seg) = tp.path.segments.last() {
                let id = seg.ident.to_string();
                if self.primitives.contains(&id) {
                    return Some(format!(
                        "`{name}` is a newtype over primitive `{id}`; use an arvo alias or named typestate"
                    ));
                }
            }
        }
        None
    }
}

impl Lint for NoManualIdLint {
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
            match item {
                syn::Item::Struct(s) if s.fields.len() == 1 => {
                    if let Some(field) = s.fields.iter().next() {
                        if let Some(msg) = self.check_inner(&s.ident.to_string(), &field.ty) {
                            emit(self.name, doc.path(), &msg, active, sink);
                        }
                    }
                }
                syn::Item::Type(t) if self.config.check_aliases => {
                    if let Some(msg) = self.check_inner(&t.ident.to_string(), &t.ty) {
                        emit(self.name, doc.path(), &msg, active, sink);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn emit(
    lint_name: &'static str,
    path: &std::path::Path,
    message: &str,
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
        message: Cow::Owned(message.to_string()),
        span: Span::single_line(path, 1, 1, 1),
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
    let parsed: NoManualIdConfig = config.clone().try_into().map_err(
        |e: toml::de::Error| ConfigError {
            lint_name: name.to_string(),
            field_path: String::new(),
            kind: ConfigErrorKind::InvalidValue,
            message: format!("no-manual-id config: {e}"),
            source_location: None,
        },
    )?;
    Ok(Box::new(NoManualIdLint::new(
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
    fn fires_on_newtype_over_u32() {
        let lint = NoManualIdLint::new(
            "no-manual-id",
            "",
            GateSeverity::uniform(Severity::Warn),
            NoManualIdConfig {
                primitive_inner_types: vec!["u32".to_string(), "u64".to_string()],
                check_aliases: true,
            },
        );
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "pub struct NodeId(u32); pub type SlotId = u64;",
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
        assert_eq!(sink.into_findings().len(), 2);
    }
}
