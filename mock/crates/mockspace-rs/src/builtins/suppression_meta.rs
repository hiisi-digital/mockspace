//! SuppressionMeta primitive.
//!
//! Per schema design memo §4.11. ProjectScoped lint that reads the
//! project-level [`SuppressionMap`] and validates it against meta-rules:
//! tracked task id presence, reason length, expired-task references,
//! per-crate overuse thresholds.
//!
//! `lint:allow(...)` is the consumer's escape hatch for individual
//! findings; this primitive ensures that hatch is not used as a routine
//! workaround. Per workspace rule `feedback_no_complex_lint_allows.md`:
//! `lint:allow` requires a linked task id and a non-trivial reason.

use std::borrow::Cow;
use std::collections::HashMap;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span, SuppressionScope};
use serde::Deserialize;

use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

pub const KIND: &str = "suppression-meta";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SuppressionMetaConfig {
    #[serde(default = "default_true")]
    pub require_tracked: bool,

    #[serde(default = "default_true")]
    pub require_reason: bool,

    #[serde(default = "default_min_reason_words")]
    pub require_reason_min_words: u32,

    #[serde(default)]
    pub forbid_expired: bool,

    /// Fires once per crate that exceeds this count of lint:allow scopes.
    /// None disables the check.
    #[serde(default)]
    pub overuse_threshold_per_crate: Option<u32>,
}

fn default_true() -> bool {
    true
}

fn default_min_reason_words() -> u32 {
    3
}

pub struct SuppressionMetaLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: SuppressionMetaConfig,
}

impl SuppressionMetaLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: SuppressionMetaConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for SuppressionMetaLint {
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
        let suppressions = project.suppressions();
        let active = ctx.active_severity();
        let mut per_crate: HashMap<String, u32> = HashMap::new();
        for scope in suppressions.scopes() {
            // Bookkeeping: count by containing-crate (derive from scope path).
            let crate_name = crate_name_from_path(&scope.scope.file);
            *per_crate.entry(crate_name).or_default() += 1;

            if self.config.require_tracked && scope.tracked.is_none() {
                emit_scope(
                    self.name,
                    scope,
                    active,
                    sink,
                    "suppression lacks `tracked: #N` task reference",
                );
            }
            if self.config.require_reason {
                let reason_words = scope
                    .reason
                    .as_deref()
                    .map(|r| r.split_whitespace().count() as u32)
                    .unwrap_or(0);
                if reason_words < self.config.require_reason_min_words {
                    emit_scope(
                        self.name,
                        scope,
                        active,
                        sink,
                        &format!(
                            "suppression reason has {reason_words} word(s); minimum is {}",
                            self.config.require_reason_min_words
                        ),
                    );
                }
            }
            if self.config.forbid_expired {
                if let Some(task_ref) = &scope.tracked {
                    let task_state = &project.workspace().task_state;
                    if task_state.is_closed(task_ref) {
                        emit_scope(
                            self.name,
                            scope,
                            active,
                            sink,
                            &format!("suppression refers to closed task `{task_ref}`"),
                        );
                    }
                }
            }
        }

        if let Some(threshold) = self.config.overuse_threshold_per_crate {
            for (crate_name, count) in per_crate {
                if count > threshold {
                    sink.emit(Finding {
                        lint_name: Cow::Borrowed(self.name),
                        rule_id: Some(Cow::Borrowed("overuse")),
                        plugin_id: None,
                        severity: active,
                        impact: None,
                        category: None,
                        message: Cow::Owned(format!(
                            "crate `{crate_name}` has {count} lint:allow scopes (threshold: {threshold})"
                        )),
                        span: Span::single_line(&crate_name, 1, 1, 1),
                        hint: None,
                        help: None,
                        suggestion: None,
                        related_spans: Vec::new(),
                        metadata: None,
                    });
                }
            }
        }
        Ok(())
    }
}

fn emit_scope(
    lint_name: &'static str,
    scope: &SuppressionScope,
    severity: Severity,
    sink: &dyn FindingSink,
    message: &str,
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
        span: scope.scope.clone(),
        hint: None,
        help: None,
        suggestion: None,
        related_spans: Vec::new(),
        metadata: None,
    });
}

fn crate_name_from_path(path: &std::path::Path) -> String {
    // Best-effort: pull the crate name from `mock/crates/<name>/` path.
    let mut found_crates = false;
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if found_crates {
            return name.to_string();
        }
        if name == "crates" {
            found_crates = true;
        }
    }
    "unknown".to_string()
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: SuppressionMetaConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("suppression-meta config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(SuppressionMetaLint::new(
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
        Gate, RunSurface, Severity, Span, SuppressionKind, SuppressionMap, SuppressionScope,
    };
    use std::collections::BTreeSet;
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

    fn project_with_suppressions(scopes: Vec<SuppressionScope>) -> MockspaceProject {
        let mut map = SuppressionMap::new();
        for s in scopes {
            map.push(s);
        }
        ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit)
            .with_suppressions(map)
            .build()
    }

    fn lint_set() -> BTreeSet<String> {
        let mut s = BTreeSet::new();
        s.insert("no-alloc".to_string());
        s
    }

    #[test]
    fn fires_on_untracked_suppression() {
        let lint = SuppressionMetaLint::new(
            "no-untracked-suppressions",
            "",
            GateSeverity::uniform(Severity::Warn),
            SuppressionMetaConfig {
                require_tracked: true,
                require_reason: false,
                require_reason_min_words: 1,
                forbid_expired: false,
                overuse_threshold_per_crate: None,
            },
        );
        let project = project_with_suppressions(vec![SuppressionScope {
            scope: Span::single_line("a.rs", 1, 1, 5),
            lints: lint_set(),
            kind: SuppressionKind::Allow,
            tracked: None,
            reason: Some("because reasons".to_string()),
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
    fn fires_on_short_reason() {
        let lint = SuppressionMetaLint::new(
            "no-trivial-reasons",
            "",
            GateSeverity::uniform(Severity::Warn),
            SuppressionMetaConfig {
                require_tracked: false,
                require_reason: true,
                require_reason_min_words: 5,
                forbid_expired: false,
                overuse_threshold_per_crate: None,
            },
        );
        let project = project_with_suppressions(vec![SuppressionScope {
            scope: Span::single_line("a.rs", 1, 1, 5),
            lints: lint_set(),
            kind: SuppressionKind::Allow,
            tracked: Some("#123".to_string()),
            reason: Some("short".to_string()),
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
    fn accepts_well_formed_suppression() {
        let lint = SuppressionMetaLint::new(
            "all-rules",
            "",
            GateSeverity::uniform(Severity::Warn),
            SuppressionMetaConfig {
                require_tracked: true,
                require_reason: true,
                require_reason_min_words: 3,
                forbid_expired: false,
                overuse_threshold_per_crate: None,
            },
        );
        let project = project_with_suppressions(vec![SuppressionScope {
            scope: Span::single_line("a.rs", 1, 1, 5),
            lints: lint_set(),
            kind: SuppressionKind::Allow,
            tracked: Some("#456".to_string()),
            reason: Some("Boundary requires raw bytes".to_string()),
        }]);
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        assert!(sink.into_findings().is_empty());
    }

    // ---- end-to-end integration test ----------------------------------
    //
    // The tests above unit-test the meta-lint by injecting suppression
    // scopes via `with_suppressions`. That tests the lint's logic but
    // skips the preprocessor → project bridge. The test below covers
    // the full pipeline: a Rust document carrying a `// lint:allow(...)`
    // comment goes through `MockspaceEngine::populate_directives` (the
    // same code path `scope_project` uses), which extracts the scope
    // into `project.suppressions()`. The meta-lint then reads from
    // there. A regression that left suppressions empty under the
    // production scope walk would surface here.

    #[test]
    fn meta_lint_sees_scope_from_real_preprocessor_run() {
        use crate::MockspaceEngine;
        use crate::document::MockspaceDocument;
        use mockspace_core::lint::Language;

        let lint = SuppressionMetaLint::new(
            "no-untracked-suppressions",
            "",
            GateSeverity::uniform(Severity::Warn),
            SuppressionMetaConfig {
                require_tracked: true,
                require_reason: false,
                require_reason_min_words: 1,
                forbid_expired: false,
                overuse_threshold_per_crate: None,
            },
        );
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        // `lint:allow` with reason but NO tracked: the meta-lint should
        // fire under `require_tracked = true`.
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "test-crate",
            Language::Rust,
            "// lint:allow(no-alloc) reason: \"intentional\"\nfn x() {}\n",
        ));
        let mut project = builder.build();
        let engine = MockspaceEngine::with_entries(Vec::new());
        engine.populate_directives(&mut project).unwrap();
        // Project should now carry the parsed suppression scope.
        assert_eq!(project.suppressions().scopes().len(), 1);

        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("tracked"));
    }
}
