//! WorkflowState primitive.
//!
//! Per schema design memo §4.10. Project-mode lint that validates the
//! `mock/design_rounds/` tree against typestate rules: locked CLs can't
//! be edited, doc CLs lock before src CLs, every design round has a doc
//! CL, filenames match the YYYYMMDDHHMM_ convention.
//!
//! Today this primitive is a thin shell: it reads `MockspaceProject::
//! design_rounds()` and validates the parsed view. The actual round
//! parsing happens at project load (DesignRoundsView). When DesignRoundsView
//! is populated by the project walker (Phase 2B), the validators here
//! report the gates the design rounds violate.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::{MockspaceProject, RoundState};

pub const KIND: &str = "workflow-state";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct WorkflowStateConfig {
    pub rule: WorkflowRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowRule {
    ChangelistLock,
    ChangelistImmutability,
    ChangelistRequired,
    ChangelistDocGate,
    DesignRoundFilenameConvention,
}

pub struct WorkflowStateLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: WorkflowStateConfig,
}

impl WorkflowStateLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: WorkflowStateConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for WorkflowStateLint {
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
        let rounds = project.design_rounds();
        let active = ctx.active_severity();
        match self.config.rule {
            WorkflowRule::ChangelistLock => {
                // Future hook: compare locked CL file content against the
                // git index. Today, this is a no-op until the engine wires
                // the git-tracked-hash inspection.
            }
            WorkflowRule::ChangelistImmutability => {
                for round in &rounds.rounds {
                    if round.state == RoundState::Locked && !round.locked {
                        emit(self.name, active, sink, &format!(
                            "round `{}` claims Locked state but `locked` flag is unset",
                            round.timestamp
                        ));
                    }
                }
            }
            WorkflowRule::ChangelistRequired => {
                for round in &rounds.rounds {
                    if round.doc_cl.is_none() && round.state != RoundState::Topic {
                        emit(self.name, active, sink, &format!(
                            "round `{}` has advanced past Topic without a doc CL",
                            round.timestamp
                        ));
                    }
                }
            }
            WorkflowRule::ChangelistDocGate => {
                for round in &rounds.rounds {
                    if round.src_cl.is_some()
                        && round.doc_cl.is_none()
                    {
                        emit(self.name, active, sink, &format!(
                            "round `{}` has src CL without doc CL",
                            round.timestamp
                        ));
                    }
                }
            }
            WorkflowRule::DesignRoundFilenameConvention => {
                for round in &rounds.rounds {
                    if !is_valid_round_timestamp(&round.timestamp) {
                        emit(self.name, active, sink, &format!(
                            "round `{}` does not match YYYYMMDDHHMM convention",
                            round.timestamp
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn emit(lint_name: &'static str, severity: Severity, sink: &dyn FindingSink, message: &str) {
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
        span: Span::single_line("mock/design_rounds", 1, 1, 1),
        fix_suggestion: None,
        related_spans: Vec::new(),
        metadata: None,
    });
}

fn is_valid_round_timestamp(ts: &str) -> bool {
    ts.len() == 12 && ts.bytes().all(|b| b.is_ascii_digit())
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: WorkflowStateConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("workflow-state config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(WorkflowStateLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{DesignRound, DesignRoundsView, ProjectBuilder};
    use crate::finding_sink::VecFindingSink;
    use mockspace_core::lint::{Gate, RunSurface, Severity};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(
        root: &'a PathBuf,
        sev: GateSeverity,
        cfg: &'a EmptyCfg,
    ) -> LintContext<'a> {
        LintContext {
            gate: Gate::Commit,
            severities: sev,
            surface: RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    fn project_with(rounds: Vec<DesignRound>) -> MockspaceProject {
        ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit)
            .with_design_rounds(DesignRoundsView {
                root: PathBuf::from("mock/design_rounds"),
                rounds,
            })
            .build()
    }

    #[test]
    fn fires_on_bad_filename_convention() {
        let lint = WorkflowStateLint::new(
            "round-filename",
            "",
            GateSeverity::uniform(Severity::Warn),
            WorkflowStateConfig {
                rule: WorkflowRule::DesignRoundFilenameConvention,
            },
        );
        let project = project_with(vec![DesignRound {
            timestamp: "bad-format".to_string(),
            state: RoundState::Topic,
            doc_cl: None,
            src_cl: None,
            locked: false,
        }]);
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
    }

    #[test]
    fn doc_gate_catches_src_without_doc() {
        let lint = WorkflowStateLint::new(
            "doc-gate",
            "",
            GateSeverity::uniform(Severity::Warn),
            WorkflowStateConfig {
                rule: WorkflowRule::ChangelistDocGate,
            },
        );
        let project = project_with(vec![DesignRound {
            timestamp: "202605211200".to_string(),
            state: RoundState::Src,
            doc_cl: None,
            src_cl: Some(PathBuf::from("src.md")),
            locked: false,
        }]);
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
    }
}
