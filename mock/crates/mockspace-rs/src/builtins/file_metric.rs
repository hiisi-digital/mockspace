//! FileMetric primitive.
//!
//! Per schema design memo §4.7. Counts lines or items in a document and
//! fires when the count crosses a configured threshold. Covers
//! `max-line-count`, `max-pub-item-count`, and similar consumer-shaped
//! rules.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Span};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "file-metric";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct FileMetricConfig {
    pub metric: Metric,
    pub threshold: u32,
    /// `true` fires when count >= threshold; `false` when count > threshold.
    #[serde(default = "default_true")]
    pub inclusive: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Metric {
    LineCount,
    NonBlankLineCount,
    NonBlankNonCommentLineCount,
    PubItemCount,
    PrivateItemCount,
    TotalItemCount,
}

pub struct FileMetricLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: FileMetricConfig,
}

impl FileMetricLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: FileMetricConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for FileMetricLint {
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
        matches!(
            self.config.metric,
            Metric::PubItemCount | Metric::PrivateItemCount | Metric::TotalItemCount
        )
    }

    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let count = compute_metric(doc, self.config.metric);
        let triggered = if self.config.inclusive {
            count >= self.config.threshold
        } else {
            count > self.config.threshold
        };
        if !triggered {
            return Ok(());
        }
        sink.emit(Finding {
            lint_name: Cow::Borrowed(self.name),
            rule_id: None,
            plugin_id: None,
            severity: ctx.active_severity(),
            impact: None,
            category: None,
            message: Cow::Owned(format!(
                "{:?} count {count} exceeds threshold {}",
                self.config.metric, self.config.threshold
            )),
            span: Span::single_line(doc.path(), 1, 1, 1),
            hint: None,
            help: None,
            suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        });
        Ok(())
    }
}

fn compute_metric(doc: &MockspaceDocument, metric: Metric) -> u32 {
    match metric {
        Metric::LineCount => doc.source().lines().count() as u32,
        Metric::NonBlankLineCount => doc
            .source()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count() as u32,
        Metric::NonBlankNonCommentLineCount => doc
            .source()
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("//")
            })
            .count() as u32,
        Metric::PubItemCount => doc
            .ast()
            .map(|f| count_items(f, |it| is_pub_item(it)))
            .unwrap_or(0),
        Metric::PrivateItemCount => doc
            .ast()
            .map(|f| count_items(f, |it| !is_pub_item(it)))
            .unwrap_or(0),
        Metric::TotalItemCount => doc.ast().map(|f| f.items.len() as u32).unwrap_or(0),
    }
}

fn count_items<F: Fn(&syn::Item) -> bool>(file: &syn::File, pred: F) -> u32 {
    file.items.iter().filter(|i| pred(i)).count() as u32
}

fn is_pub_item(item: &syn::Item) -> bool {
    match item {
        syn::Item::Fn(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Struct(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Enum(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Trait(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Type(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Const(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Static(it) => matches!(it.vis, syn::Visibility::Public(_)),
        syn::Item::Mod(it) => matches!(it.vis, syn::Visibility::Public(_)),
        _ => false,
    }
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: FileMetricConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("file-metric config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(FileMetricLint::new(
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
    use mockspace_core::lint::{Gate, Severity};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(root: &'a PathBuf, sev: GateSeverity, cfg: &'a EmptyCfg) -> LintContext<'a> {
        LintContext {
            gate: Gate::Commit,
            severities: sev,
            surface: mockspace_core::lint::RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    #[test]
    fn line_count_triggers() {
        let lint = FileMetricLint::new(
            "max-lines",
            "",
            GateSeverity::uniform(Severity::Warn),
            FileMetricConfig {
                metric: Metric::LineCount,
                threshold: 3,
                inclusive: true,
            },
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "a\nb\nc\n");
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn pub_item_count_uses_ast() {
        let lint = FileMetricLint::new(
            "max-pub",
            "",
            GateSeverity::uniform(Severity::Warn),
            FileMetricConfig {
                metric: Metric::PubItemCount,
                threshold: 2,
                inclusive: true,
            },
        );
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "pub fn a() {} pub fn b() {} fn c() {}",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
        assert!(lint.needs_syn_ast());
    }

    #[test]
    fn does_not_fire_below_threshold() {
        let lint = FileMetricLint::new(
            "max-lines",
            "",
            GateSeverity::uniform(Severity::Warn),
            FileMetricConfig {
                metric: Metric::LineCount,
                threshold: 100,
                inclusive: true,
            },
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "a\nb");
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert!(sink.into_findings().is_empty());
    }
}
