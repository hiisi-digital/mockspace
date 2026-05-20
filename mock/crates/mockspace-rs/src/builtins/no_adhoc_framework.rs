//! `no_adhoc_framework` bespoke primitive.
//!
//! Per schema design memo §4.15. Heuristic detector for ad-hoc framework
//! patterns: dispatch tables (struct with fn-pointer fields + central
//! dispatcher), init/run/cleanup triples, callback-chain patterns. Lives
//! in the bespoke bucket precisely because the tuning is one-off per
//! consumer and the signal is coarse.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

pub const KIND: &str = "no-adhoc-framework";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct NoAdhocFrameworkConfig {
    #[serde(default = "default_true")]
    pub detect_dispatch_tables: bool,
    #[serde(default = "default_true")]
    pub detect_lifecycle_triples: bool,
    #[serde(default = "default_true")]
    pub detect_callback_chains: bool,
    /// Minimum signal count to fire (heuristic threshold).
    #[serde(default = "default_min_signal")]
    pub min_signal_count: u32,
}

fn default_true() -> bool {
    true
}

fn default_min_signal() -> u32 {
    3
}

pub struct NoAdhocFrameworkLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: NoAdhocFrameworkConfig,
}

impl NoAdhocFrameworkLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: NoAdhocFrameworkConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for NoAdhocFrameworkLint {
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

    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let active = ctx.active_severity();
        if active.silent() {
            return Ok(());
        }
        // Scan all Rust files for the three heuristic signals.
        let mut dispatch_signal: u32 = 0;
        let mut lifecycle_signal: u32 = 0;
        let mut callback_signal: u32 = 0;

        for doc in project.documents() {
            let Some(file) = doc.ast() else {
                continue;
            };
            for item in &file.items {
                if self.config.detect_dispatch_tables {
                    if let syn::Item::Struct(s) = item {
                        let fn_ptr_fields = s
                            .fields
                            .iter()
                            .filter(|f| matches!(&f.ty, syn::Type::BareFn(_)))
                            .count() as u32;
                        if fn_ptr_fields >= 2 {
                            dispatch_signal = dispatch_signal.saturating_add(fn_ptr_fields);
                        }
                    }
                }
                if self.config.detect_lifecycle_triples {
                    if let syn::Item::Impl(impl_block) = item {
                        let mut has_init = false;
                        let mut has_run = false;
                        let mut has_cleanup = false;
                        for impl_item in &impl_block.items {
                            if let syn::ImplItem::Fn(m) = impl_item {
                                let n = m.sig.ident.to_string();
                                if matches!(n.as_str(), "init" | "initialize" | "setup") {
                                    has_init = true;
                                }
                                if matches!(n.as_str(), "run" | "execute" | "tick") {
                                    has_run = true;
                                }
                                if matches!(n.as_str(), "cleanup" | "teardown" | "shutdown" | "destroy") {
                                    has_cleanup = true;
                                }
                            }
                        }
                        if has_init && has_run && has_cleanup {
                            lifecycle_signal = lifecycle_signal.saturating_add(1);
                        }
                    }
                }
                if self.config.detect_callback_chains {
                    if let syn::Item::Fn(f) = item {
                        let callback_params = f
                            .sig
                            .inputs
                            .iter()
                            .filter(|i| match i {
                                syn::FnArg::Typed(pt) => matches!(
                                    &*pt.ty,
                                    syn::Type::BareFn(_) | syn::Type::ImplTrait(_)
                                ),
                                _ => false,
                            })
                            .count() as u32;
                        if callback_params >= 2 {
                            callback_signal = callback_signal.saturating_add(callback_params);
                        }
                    }
                }
            }
        }

        let total = dispatch_signal
            .saturating_add(lifecycle_signal)
            .saturating_add(callback_signal);
        if total >= self.config.min_signal_count {
            sink.emit(Finding {
                lint_name: Cow::Borrowed(self.name),
                rule_id: Some(Cow::Borrowed("adhoc-framework")),
                plugin_id: None,
                severity: active,
                impact: None,
                category: None,
                message: Cow::Owned(format!(
                    "ad-hoc framework heuristics fired: dispatch={dispatch_signal}, lifecycle={lifecycle_signal}, callback={callback_signal}; use hilavitkutin scheduler instead"
                )),
                span: Span::single_line("project", 1, 1, 1),
                fix_suggestion: None,
                related_spans: Vec::new(),
                metadata: None,
            });
        }
        Ok(())
    }
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: NoAdhocFrameworkConfig = if config.is_empty() {
        NoAdhocFrameworkConfig {
            detect_dispatch_tables: true,
            detect_lifecycle_triples: true,
            detect_callback_chains: true,
            min_signal_count: default_min_signal(),
        }
    } else {
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("no-adhoc-framework config: {e}"),
                source_location: None,
            })?
    };
    Ok(Box::new(NoAdhocFrameworkLint::new(
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
    use crate::project::ProjectBuilder;
    use mockspace_core::lint::{Gate, RunSurface};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    #[test]
    fn fires_when_signals_exceed_threshold() {
        let lint = NoAdhocFrameworkLint::new(
            "no-adhoc-framework",
            "",
            GateSeverity::uniform(Severity::Warn),
            NoAdhocFrameworkConfig {
                detect_dispatch_tables: true,
                detect_lifecycle_triples: true,
                detect_callback_chains: true,
                min_signal_count: 1,
            },
        );
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "struct S { a: fn(), b: fn() }",
        ));
        let project = builder.build();
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
        lint.check_project(&ctx, &project, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
    }
}
