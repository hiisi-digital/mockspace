//! `directive-style-consistency`: enforces project-wide uniformity of
//! directive surface (comment-form vs attribute-form).
//!
//! Per the canonical directive vocabulary memo at
//! `mock/research/202605220000_canonical-directive-vocabulary.md`:
//!
//! - `comments-only`: every directive must be in comment form
//!   (`// lint:allow(...)`). Attribute usage is a finding.
//! - `attributes-when-available`: in languages with a native attribute
//!   parser, every directive must use the native form
//!   (`#[mockspace::allow(...)]`). Comment usage in those languages is
//!   a finding. Other languages still use comments.
//! - `mixed`: both forms accepted, no consistency check.
//!
//! The lint reads `MockspaceProject::directive_records()` which the
//! preprocessor populates with `source_form` preserved per record.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, SourceForm};
use serde::Deserialize;

use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

pub const KIND: &str = "directive-style-consistency";

/// Configured project-wide directive style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
    /// Every directive must be in comment form. Attribute usage fires.
    CommentsOnly,
    /// Every directive in a language with a native attribute parser
    /// must use the attribute form. Comment usage fires.
    AttributesWhenAvailable,
    /// Both forms accepted; no consistency check.
    Mixed,
}

impl Default for Style {
    fn default() -> Self {
        // Per memo: `comments-only` while only the comment parser
        // exists project-wide. Once the attribute parser is integrated
        // (#545, done), this default still keeps the lint silent for
        // consumers who haven't opted into a stricter style.
        Style::Mixed
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct DirectiveStyleConsistencyConfig {
    #[serde(default)]
    pub style: Style,
}

pub struct DirectiveStyleConsistencyLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: DirectiveStyleConsistencyConfig,
}

impl DirectiveStyleConsistencyLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: DirectiveStyleConsistencyConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for DirectiveStyleConsistencyLint {
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
        let active = ctx.active_severity();
        if active.silent() {
            return Ok(());
        }

        // `mixed` accepts everything: short-circuit.
        let forbidden = match self.config.style {
            Style::Mixed => return Ok(()),
            Style::CommentsOnly => SourceForm::Attribute,
            Style::AttributesWhenAvailable => SourceForm::Comment,
        };

        let policy_msg = match self.config.style {
            Style::CommentsOnly => {
                "directive uses attribute form, but project policy is `comments-only`"
            }
            Style::AttributesWhenAvailable => {
                "directive uses comment form, but project policy is `attributes-when-available`"
            }
            Style::Mixed => unreachable!("mixed short-circuits above"),
        };

        for record in project.directive_records() {
            if record.source_form == forbidden {
                sink.emit(Finding {
                    lint_name: Cow::Borrowed(self.name),
                    rule_id: None,
                    plugin_id: None,
                    severity: active,
                    impact: None,
                    category: None,
                    message: Cow::Borrowed(policy_msg),
                    span: record.span.clone(),
                    hint: None,
                    help: None,
                    suggestion: None,
                    related_spans: Vec::new(),
                    metadata: None,
                });
            }
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
    let parsed: DirectiveStyleConsistencyConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("directive-style-consistency config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(DirectiveStyleConsistencyLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding_sink::VecFindingSink;
    use crate::project::ProjectBuilder;
    use mockspace_core::lint::{
        ContentHash, Directive, DirectiveRecord, Gate, RunSurface, Severity, SourceForm, Span,
    };
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
            surface: RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    fn rec(form: SourceForm, lint_name: &str) -> DirectiveRecord {
        DirectiveRecord {
            directive: Directive::Allow {
                lint_name: lint_name.to_string(),
                reason: Some("test".to_string()),
                tracked: Some("#1".to_string()),
            },
            span: Span::single_line("a.rs", 1, 0, 30),
            source_form: form,
        }
    }

    fn project_with_records(records: Vec<DirectiveRecord>) -> MockspaceProject {
        let mut p = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit).build();
        p.directive_records = records;
        p
    }

    fn run_lint(style: Style, records: Vec<DirectiveRecord>) -> Vec<Finding> {
        let lint = DirectiveStyleConsistencyLint::new(
            "directive-style-consistency",
            "test",
            GateSeverity::uniform(Severity::Warn),
            DirectiveStyleConsistencyConfig { style },
        );
        let project = project_with_records(records);
        let root = PathBuf::from("/tmp");
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, GateSeverity::uniform(Severity::Warn), &cfg);
        let sink = VecFindingSink::default();
        lint.check_project(&ctx, &project, &sink).unwrap();
        sink.into_findings()
    }

    #[test]
    fn comments_only_fires_on_attribute_record() {
        let findings = run_lint(
            Style::CommentsOnly,
            vec![rec(SourceForm::Attribute, "no-bare-numeric")],
        );
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("comments-only"));
    }

    #[test]
    fn comments_only_silent_on_comment_record() {
        let findings = run_lint(
            Style::CommentsOnly,
            vec![rec(SourceForm::Comment, "no-bare-numeric")],
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn attributes_when_available_fires_on_comment_record() {
        let findings = run_lint(
            Style::AttributesWhenAvailable,
            vec![rec(SourceForm::Comment, "no-bare-numeric")],
        );
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("attributes-when-available"));
    }

    #[test]
    fn attributes_when_available_silent_on_attribute_record() {
        let findings = run_lint(
            Style::AttributesWhenAvailable,
            vec![rec(SourceForm::Attribute, "no-bare-numeric")],
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn mixed_silent_on_both_forms() {
        let findings = run_lint(
            Style::Mixed,
            vec![
                rec(SourceForm::Comment, "no-bare-numeric"),
                rec(SourceForm::Attribute, "no-bare-string"),
            ],
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn comments_only_fires_per_violating_record() {
        let findings = run_lint(
            Style::CommentsOnly,
            vec![
                rec(SourceForm::Attribute, "no-bare-numeric"),
                rec(SourceForm::Comment, "no-bare-string"),
                rec(SourceForm::Attribute, "no-alloc"),
            ],
        );
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn empty_records_yields_no_findings() {
        for style in [
            Style::CommentsOnly,
            Style::AttributesWhenAvailable,
            Style::Mixed,
        ] {
            let findings = run_lint(style, vec![]);
            assert!(
                findings.is_empty(),
                "style {style:?} should not fire on empty records"
            );
        }
    }
}
