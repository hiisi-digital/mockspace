//! `MockspaceEngine`: concrete lint engine using the catalog-based
//! dispatch model from the schema design memo.
//!
//! The engine holds a vector of [`InstantiatedLint`]s (each pairing a
//! `Box<dyn Lint>` with its catalog `mode` / `staging_aware` /
//! `editor_skip` metadata) plus per-language preprocessors. `run` walks
//! the lints, routes each by `LintMode`, and emits findings through a
//! shared [`VecFindingSink`]. Suppression filtering applies before
//! returning.

use std::path::Path;

use mockspace_core::lint::{
    Finding, Gate, HashAlgorithm, Language, LintCfgStore, LintContext, LintEngine,
    RunSurface, SuppressionMap,
};

use crate::config_loader::{InstantiatedLint, LintsConfig};
use crate::errors::{DispatchError, LoadError, ParseError};
use crate::finding_sink::VecFindingSink;
use crate::lint::LintMode;
use crate::preprocessor::{LanguagePreprocessor, RustPreprocessor};
use crate::project::MockspaceProject;
use crate::scope::scope_walk;

/// The Rust lint engine.
pub struct MockspaceEngine {
    lints: Vec<InstantiatedLint>,
    rust_preprocessor: RustPreprocessor,
}

impl std::fmt::Debug for MockspaceEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockspaceEngine")
            .field("lints", &self.lints.len())
            .finish()
    }
}

impl MockspaceEngine {
    /// Build with an explicit lint set. Useful for tests; the production
    /// path constructs via [`Self::new`] which uses the registered catalog.
    pub fn with_entries(entries: Vec<InstantiatedLint>) -> Self {
        Self {
            lints: entries,
            rust_preprocessor: RustPreprocessor,
        }
    }

    /// Load from the registered catalog with default configs.
    pub fn from_catalog_defaults() -> Result<Self, LoadError> {
        let config = LintsConfig::from_catalog_defaults();
        if !config.config_errors.is_empty() {
            return Err(LoadError::Config(config.config_errors));
        }
        Ok(Self::with_entries(config.entries))
    }

    /// Walk every document through the matching preprocessor and collect
    /// suppression scopes.
    fn extract_suppressions(
        &self,
        project: &MockspaceProject,
    ) -> Result<SuppressionMap, DispatchError> {
        let mut map = SuppressionMap::new();
        for doc in project.documents() {
            if doc.language() == Language::Rust {
                self.rust_preprocessor.extract(doc, &mut map).map_err(|e| {
                    DispatchError::RuntimeRefused {
                        reason: format!("preprocessor failed: {e}"),
                    }
                })?;
            }
        }
        Ok(map)
    }
}

impl LintEngine for MockspaceEngine {
    type Project = MockspaceProject;
    type ParseError = ParseError;
    type LoadError = LoadError;
    type DispatchError = DispatchError;

    const HASH_ALGORITHM: HashAlgorithm = HashAlgorithm::Blake3;

    fn new() -> Result<Self, Self::LoadError> {
        Self::from_catalog_defaults()
    }

    fn scope_project(
        &self,
        root: &Path,
        surface: RunSurface,
    ) -> Result<Self::Project, Self::ParseError> {
        scope_walk(root, surface)
    }

    fn run(
        &self,
        project: &Self::Project,
        gate: Gate,
        cfg: &dyn LintCfgStore,
    ) -> Result<Vec<Finding>, Self::DispatchError> {
        let suppressions = self.extract_suppressions(project)?;
        let sink = VecFindingSink::new();

        for entry in &self.lints {
            let severities = cfg
                .resolve_severity(entry.lint.name())
                .unwrap_or_else(|| entry.lint.default_severity());

            if severities.at(gate).silent() {
                continue;
            }

            if project.surface() == RunSurface::Editor && entry.editor_skip {
                continue;
            }

            let ctx = LintContext {
                gate,
                severities,
                surface: project.surface(),
                project_root: project.root(),
                config: cfg,
            };

            // PerDocument iteration scope: full documents() by default;
            // staged_documents() when the lint is staging_aware AND the
            // gate-specific only_staged flag is set. Editor surface marks
            // every document staged (see ProjectBuilder::mark_all_staged),
            // so commit-gate parity is preserved at editor-time.
            let only_staged_here =
                entry.staging_aware && entry.only_staged.at(gate);

            match entry.mode {
                LintMode::PerDocument => {
                    if only_staged_here {
                        for doc in project.staged_documents() {
                            entry.lint.check_document(&ctx, doc, &sink).map_err(|source| {
                                DispatchError::LintErrored {
                                    lint_name: entry.lint.name().to_owned(),
                                    source,
                                }
                            })?;
                        }
                    } else {
                        for doc in project.documents() {
                            entry.lint.check_document(&ctx, doc, &sink).map_err(|source| {
                                DispatchError::LintErrored {
                                    lint_name: entry.lint.name().to_owned(),
                                    source,
                                }
                            })?;
                        }
                    }
                }
                LintMode::ProjectScoped | LintMode::TwoPhaseProject => {
                    entry.lint.check_project(&ctx, project, &sink).map_err(|source| {
                        DispatchError::LintErrored {
                            lint_name: entry.lint.name().to_owned(),
                            source,
                        }
                    })?;
                }
            }
        }

        let findings = sink.into_findings();
        let filtered: Vec<Finding> = findings
            .into_iter()
            .filter(|f| suppressions.resolves(&f.lint_name, &f.span).is_none())
            .collect();
        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::token_scan::{TokenScanConfig, TokenScanLint};
    use crate::config_loader::InstantiatedLint;
    use crate::document::MockspaceDocument;
    use crate::project::ProjectBuilder;
    use mockspace_core::lint::{GateSeverity, Severity};

    struct EmptyCfg;
    impl LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    #[test]
    fn engine_with_empty_catalog_emits_nothing() {
        let engine = MockspaceEngine::with_entries(Vec::new());
        let project = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit).build();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Commit, &cfg).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn engine_dispatches_per_document_lint() {
        let config = TokenScanConfig {
            tokens: vec!["BANNED".to_string()],
            word_boundary: true,
            strip_strings: true,
            strip_comments: true,
            strip_doc_comments: true,
            severity_escalation: None,
        };
        let lint = TokenScanLint::new(
            "no-banned",
            "ban the BANNED token",
            config,
            GateSeverity::uniform(Severity::Warn),
        );
        let entries = vec![InstantiatedLint {
            lint: Box::new(lint),
            mode: LintMode::PerDocument,
            staging_aware: true,
            editor_skip: false,
            only_staged: crate::config_loader::OnlyStaged::default(),
        }];
        let engine = MockspaceEngine::with_entries(entries);

        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "test-crate",
            Language::Rust,
            "fn x() { let _ = BANNED; }",
        ));
        builder.push_document(MockspaceDocument::new(
            "b.rs",
            "test-crate",
            Language::Rust,
            "fn y() {}",
        ));
        let project = builder.build();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Commit, &cfg).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("BANNED"));
    }

    #[test]
    fn engine_skips_silent_severities() {
        let lint = TokenScanLint::new(
            "off-lint",
            "",
            TokenScanConfig {
                tokens: vec!["X".to_string()],
                ..Default::default()
            },
            GateSeverity::uniform(Severity::Off),
        );
        let entries = vec![InstantiatedLint {
            lint: Box::new(lint),
            mode: LintMode::PerDocument,
            staging_aware: false,
            editor_skip: false,
            only_staged: crate::config_loader::OnlyStaged::default(),
        }];
        let engine = MockspaceEngine::with_entries(entries);
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "X",
        ));
        let project = builder.build();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Commit, &cfg).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn engine_skips_editor_marked_lints() {
        let lint = TokenScanLint::new(
            "skip-in-editor",
            "",
            TokenScanConfig {
                tokens: vec!["X".to_string()],
                ..Default::default()
            },
            GateSeverity::uniform(Severity::Warn),
        );
        let entries = vec![InstantiatedLint {
            lint: Box::new(lint),
            mode: LintMode::TwoPhaseProject,
            staging_aware: false,
            editor_skip: true,
            only_staged: crate::config_loader::OnlyStaged::default(),
        }];
        let engine = MockspaceEngine::with_entries(entries);
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Editor, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "X",
        ));
        let project = builder.build();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Commit, &cfg).unwrap();
        assert!(findings.is_empty());
    }
}
