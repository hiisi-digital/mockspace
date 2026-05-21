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
    FileDisableSet, Finding, Gate, HashAlgorithm, IntroducerMap, Language, LintCfgStore,
    LintContext, LintEngine, RunSurface, ScopeAddMap, SuppressionMap,
};
use crate::preprocessor::PreprocessorError;

use crate::config_loader::{InstantiatedLint, LintsConfig, detect_prop_name_conflicts};
use crate::errors::{
    DirectiveValidationError, DispatchError, LoadError, ParseError, StartupWarning,
};
use crate::finding_sink::VecFindingSink;
use crate::lint::LintMode;
use crate::preprocessor::{LanguagePreprocessor, RustPreprocessor};
use crate::project::MockspaceProject;
use crate::scope::scope_walk;

/// The Rust lint engine.
pub struct MockspaceEngine {
    lints: Vec<InstantiatedLint>,
    rust_preprocessor: RustPreprocessor,
    startup_warnings: Vec<StartupWarning>,
}

impl std::fmt::Debug for MockspaceEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockspaceEngine")
            .field("lints", &self.lints.len())
            .field("startup_warnings", &self.startup_warnings.len())
            .finish()
    }
}

impl MockspaceEngine {
    /// Build with an explicit lint set. Useful for tests; the production
    /// path constructs via [`Self::new`] which uses the registered catalog.
    ///
    /// Runs [`detect_prop_name_conflicts`] over the supplied entries so
    /// the namespace-conflict check fires regardless of which constructor
    /// path the engine is built through. Test entries that exercise the
    /// detection logic land warnings here too; tests that want a clean
    /// engine pass entries whose `declared_props()` do not collide.
    pub fn with_entries(entries: Vec<InstantiatedLint>) -> Self {
        let startup_warnings = detect_prop_name_conflicts(&entries);
        Self {
            lints: entries,
            rust_preprocessor: RustPreprocessor,
            startup_warnings,
        }
    }

    /// Load from the registered catalog with default configs.
    pub fn from_catalog_defaults() -> Result<Self, LoadError> {
        let config = LintsConfig::from_catalog_defaults();
        if !config.config_errors.is_empty() {
            return Err(LoadError::Config(config.config_errors));
        }
        Ok(Self {
            lints: config.entries,
            rust_preprocessor: RustPreprocessor,
            startup_warnings: config.startup_warnings,
        })
    }

    /// Non-fatal observations made at engine construction. Empty when
    /// no conflicts were detected. CLI surfaces these alongside findings
    /// so consumers see when their lint set has overlapping prop
    /// declarations without blocking the run.
    pub fn startup_warnings(&self) -> &[StartupWarning] {
        &self.startup_warnings
    }

    /// Validate the directives resolved on the project against the
    /// registered lint catalog. Hard-fails when a directive names a
    /// lint not in the catalog or a category no registered lint
    /// declares. Per task #547.
    ///
    /// Collects ALL errors into one vector so CI users see every
    /// issue in one pass; the gate returns `Err` only when the
    /// vector is non-empty.
    ///
    /// Called from [`Self::scope_project`] after
    /// [`Self::populate_directives`]; exposed publicly so test
    /// fixtures and downstream tools can run the gate explicitly.
    pub fn validate_directives(
        &self,
        project: &MockspaceProject,
    ) -> Result<(), Vec<DirectiveValidationError>> {
        use std::collections::BTreeSet;
        // Build registry from the loaded lint set.
        let known_lints: BTreeSet<&str> =
            self.lints.iter().map(|e| e.lint.name()).collect();

        let mut errors = Vec::new();

        // Suppression scopes cover both `lint:allow` (kind Allow) and
        // `lint:defer` (kind Defer); the wire directive name shown in
        // diagnostics depends on the scope's kind.
        for scope in project.suppressions().scopes() {
            let directive_label = match scope.kind {
                mockspace_core::lint::SuppressionKind::Allow => "lint:allow",
                mockspace_core::lint::SuppressionKind::Defer => "lint:defer",
            };
            for name in &scope.lints {
                if !known_lints.contains(name.as_str()) {
                    errors.push(DirectiveValidationError::UnknownLintName {
                        directive: directive_label,
                        name: name.clone(),
                        span: scope.scope.clone(),
                    });
                }
            }
        }

        // ScopeAdd entries name a lint and a value; lint name validates here.
        for entry in project.scope_adds().entries() {
            if !known_lints.contains(entry.lint_name.as_str()) {
                errors.push(DirectiveValidationError::UnknownLintName {
                    directive: "lint:scope-add",
                    name: entry.lint_name.clone(),
                    span: entry.scope.clone(),
                });
            }
        }

        // FileDisable entries carry the directive comment's source
        // span (added alongside the validation gate in #547). The
        // diagnostic points straight at the offending
        // `// lint:file-disable(...)` line so CI users can jump to it.
        for entry in project.file_disables().entries() {
            if !known_lints.contains(entry.lint_name.as_str()) {
                errors.push(DirectiveValidationError::UnknownLintName {
                    directive: "lint:file-disable",
                    name: entry.lint_name.clone(),
                    span: entry.directive_span.clone(),
                });
            }
        }

        // No category validation: `lint:introduces(<category>)` is a
        // legacy directive scheduled for retirement. The per-site
        // exemption it provided is now written as
        // `// lint:allow(<lint-name>) reason: "..." tracked: #N`
        // directly. While `Directive::Introduces` and
        // [`mockspace_core::lint::IntroducerMap`] still exist for the
        // benefit of consumers mid-migration, the validation gate
        // does not enforce category-name consistency. Retirement
        // lands in a follow-up PR that drops the directive variant.

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Walk every Rust document through the preprocessor and stash
    /// the resolved directive state on the project. Called at
    /// [`Self::scope_project`] time on the production path. Exposed
    /// publicly so test fixtures that build projects via
    /// [`crate::project::ProjectBuilder`] (rather than `scope_walk`)
    /// can still exercise the real extraction pipeline instead of
    /// injecting suppressions externally.
    pub fn populate_directives(
        &self,
        project: &mut MockspaceProject,
    ) -> Result<(), PreprocessorError> {
        let (suppressions, introducers, scope_adds, file_disables) =
            self.aggregate_directives(project)?;
        project.set_resolved_directives(
            suppressions,
            introducers,
            scope_adds,
            file_disables,
        );
        Ok(())
    }

    fn aggregate_directives(
        &self,
        project: &MockspaceProject,
    ) -> Result<
        (SuppressionMap, IntroducerMap, ScopeAddMap, FileDisableSet),
        PreprocessorError,
    > {
        let mut suppressions = SuppressionMap::new();
        let mut introducers = IntroducerMap::new();
        let mut scope_adds = ScopeAddMap::new();
        let mut file_disables = FileDisableSet::new();
        for doc in project.documents() {
            if doc.language() == Language::Rust {
                let extracts = self.rust_preprocessor.extract(doc)?;
                for scope in extracts.suppressions.scopes() {
                    suppressions.push(scope.clone());
                }
                let intro_pairs: Vec<(_, String)> = extracts
                    .introducers
                    .entries()
                    .map(|(span, cat)| (span.clone(), cat.to_string()))
                    .collect();
                for (span, category) in intro_pairs {
                    introducers.push(span, category);
                }
                for entry in extracts.scope_adds.entries() {
                    scope_adds.push(entry.clone());
                }
                for entry in extracts.file_disables.entries() {
                    file_disables.push(entry.clone());
                }
            }
        }
        Ok((suppressions, introducers, scope_adds, file_disables))
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
        let mut project = scope_walk(root, surface)?;
        // Resolve directives inline so consumer lints reading
        // `project.suppressions()` / `project.file_disables()` etc.
        // see the actual project state, not empty defaults. The
        // `SuppressionMetaLint` and the future bare-primitive lint
        // family both depend on this; without it they see no scopes
        // and silently produce no findings. Building this state in
        // `run` (the prior shape) hid the gap because tests injected
        // via `with_suppressions`. Resolving here is the contract.
        self.populate_directives(&mut project)
            .map_err(|e| ParseError::Preprocessor {
                message: e.to_string(),
            })?;
        // Post-extraction validation gate (#547): reject the project
        // outright if any directive names an unknown lint or category.
        // Runs after population so every directive across every
        // document is visible at once and CI sees the full list of
        // failures rather than only the first.
        self.validate_directives(&project)
            .map_err(|errors| ParseError::DirectiveValidation { errors })?;
        Ok(project)
    }

    fn run(
        &self,
        project: &Self::Project,
        gate: Gate,
        cfg: &dyn LintCfgStore,
    ) -> Result<Vec<Finding>, Self::DispatchError> {
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
                    // Per-document scope filter: each lint's ScopeConfig
                    // is pre-compiled into `entry.scope_filter`. Documents
                    // that fail the filter are silently skipped; the lint
                    // never sees them.
                    //
                    // Sequential today. The schema design memo §15 calls
                    // for rayon parallelism here, but MockspaceDocument's
                    // lazy syn / tree-sitter AST caches don't propagate
                    // Sync (the inner crates' types aren't Sync). Wiring
                    // rayon requires either Mutex-wrapping the caches or
                    // an `unsafe impl Sync` with a documented post-init
                    // read-only invariant. Tracked as a follow-up.
                    //
                    // Branches duplicate the inner closure rather than
                    // boxing the iterator: `Box<dyn Iterator<...>>` heap-
                    // allocates per-(lint, gate) on every dispatch.
                    let dispatch = |doc: &crate::document::MockspaceDocument| {
                        if !entry.scope_filter.accepts(doc, project) {
                            return Ok(());
                        }
                        entry.lint.check_document(&ctx, doc, &sink).map_err(|source| {
                            DispatchError::LintErrored {
                                lint_name: entry.lint.name().to_owned(),
                                source,
                            }
                        })
                    };
                    if only_staged_here {
                        for doc in project.staged_documents() {
                            dispatch(doc)?;
                        }
                    } else {
                        for doc in project.documents() {
                            dispatch(doc)?;
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
        let file_disables = project.file_disables();
        let suppressions = project.suppressions();
        let filtered: Vec<Finding> = findings
            .into_iter()
            // File-disable filtering runs before span-bounded
            // suppression resolution: a file-disable kills every
            // finding in the file regardless of span; suppression
            // resolution only fires if the file did not silence the
            // lint outright. Both apply.
            .filter(|f| !file_disables.disabled(&f.span.file, &f.lint_name))
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
            scope_filter: crate::scope_filter::ScopeFilter::from_config("test", &crate::config_types::ScopeConfig::default()).unwrap(),
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
            scope_filter: crate::scope_filter::ScopeFilter::from_config("test", &crate::config_types::ScopeConfig::default()).unwrap(),
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
    fn engine_surfaces_prop_name_conflict_via_startup_warnings() {
        use crate::errors::StartupWarning;
        use crate::lint::Lint;

        struct PropLint {
            name: &'static str,
        }
        impl Lint for PropLint {
            fn name(&self) -> &'static str {
                self.name
            }
            fn description(&self) -> &'static str {
                "props stub"
            }
            fn default_severity(&self) -> GateSeverity {
                GateSeverity::uniform(Severity::Warn)
            }
            fn declared_props(&self) -> &'static [&'static str] {
                &["audited"]
            }
        }
        let make = |name: &'static str| InstantiatedLint {
            lint: Box::new(PropLint { name }),
            mode: LintMode::PerDocument,
            staging_aware: false,
            editor_skip: false,
            only_staged: crate::config_loader::OnlyStaged::default(),
            scope_filter: crate::scope_filter::ScopeFilter::from_config(
                name,
                &crate::config_types::ScopeConfig::default(),
            )
            .unwrap(),
        };
        let engine = MockspaceEngine::with_entries(vec![make("lint-a"), make("lint-b")]);
        let warnings = engine.startup_warnings();
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            StartupWarning::PropNameConflict { prop_name, lints } => {
                assert_eq!(prop_name, "audited");
                assert_eq!(lints, &vec!["lint-a".to_string(), "lint-b".to_string()]);
            }
        }
    }

    #[test]
    fn engine_file_disable_directive_drops_findings_in_that_file() {
        // A `// lint:file-disable(no-banned)` in a Rust source file
        // suppresses every `no-banned` finding emitted against that
        // file, regardless of where in the file the finding lands.
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
            scope_filter: crate::scope_filter::ScopeFilter::from_config(
                "test",
                &crate::config_types::ScopeConfig::default(),
            )
            .unwrap(),
        }];
        let engine = MockspaceEngine::with_entries(entries);

        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        // File whose lint:file-disable should suppress the finding.
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "test-crate",
            Language::Rust,
            "// lint:file-disable(no-banned) reason: \"fixture\" tracked: #1\nfn x() { let _ = BANNED; }",
        ));
        // Second file without the disable directive: finding survives.
        builder.push_document(MockspaceDocument::new(
            "b.rs",
            "test-crate",
            Language::Rust,
            "fn y() { let _ = BANNED; }",
        ));
        let mut project = builder.build();
        // Exercise the real preprocessor pipeline rather than
        // injecting state via `with_suppressions`. The engine's
        // production path calls this from `scope_project`; tests
        // that build via `ProjectBuilder` (instead of `scope_walk`)
        // call it explicitly so they exercise the same code path.
        engine.populate_directives(&mut project).unwrap();
        let cfg = EmptyCfg;
        let findings = engine.run(&project, Gate::Commit, &cfg).unwrap();
        // a.rs finding is dropped by the file-disable; b.rs finding survives.
        assert_eq!(findings.len(), 1);
        assert!(findings[0].span.file.ends_with("b.rs"));
    }

    #[test]
    fn engine_with_no_prop_conflicts_has_empty_startup_warnings() {
        let engine = MockspaceEngine::with_entries(Vec::new());
        assert!(engine.startup_warnings().is_empty());
    }

    // ---- directive validation gate (#547) ----------------------------

    /// Build an `InstantiatedLint` whose `Lint` impl reports the given
    /// name and an optional set of declared categories. Used by the
    /// validation-gate tests below to spin up a catalog that knows
    /// about specific lints/categories without registering full lint
    /// machinery via inventory.
    fn stub_lint_entry(name: &'static str) -> InstantiatedLint {
        use crate::lint::Lint;
        struct StubLint {
            name: &'static str,
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
        }
        InstantiatedLint {
            lint: Box::new(StubLint { name }),
            mode: LintMode::PerDocument,
            staging_aware: false,
            editor_skip: false,
            only_staged: crate::config_loader::OnlyStaged::default(),
            scope_filter: crate::scope_filter::ScopeFilter::from_config(
                name,
                &crate::config_types::ScopeConfig::default(),
            )
            .unwrap(),
        }
    }

    #[test]
    fn validation_gate_passes_when_all_directives_name_known_lints() {
        let engine = MockspaceEngine::with_entries(vec![
            stub_lint_entry("no-banned"),
            stub_lint_entry("no-bare-numeric"),
        ]);
        let mut builder =
            ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "c",
            Language::Rust,
            "// lint:allow(no-banned) reason: \"x\" tracked: #1\nfn x() {}\n",
        ));
        let mut project = builder.build();
        engine.populate_directives(&mut project).unwrap();
        engine.validate_directives(&project).unwrap();
    }

    #[test]
    fn validation_gate_rejects_unknown_lint_name_in_allow() {
        let engine = MockspaceEngine::with_entries(vec![stub_lint_entry("known")]);
        let mut builder =
            ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "c",
            Language::Rust,
            "// lint:allow(unknown-lint) reason: \"x\" tracked: #1\nfn x() {}\n",
        ));
        let mut project = builder.build();
        engine.populate_directives(&mut project).unwrap();
        let err = engine.validate_directives(&project).unwrap_err();
        assert_eq!(err.len(), 1);
        match &err[0] {
            DirectiveValidationError::UnknownLintName { directive, name, .. } => {
                assert_eq!(*directive, "lint:allow");
                assert_eq!(name, "unknown-lint");
            }
        }
    }

    #[test]
    fn validation_gate_rejects_unknown_lint_in_defer_and_file_disable_and_scope_add() {
        let engine = MockspaceEngine::with_entries(vec![stub_lint_entry("known")]);
        let mut builder =
            ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "c",
            Language::Rust,
            "// lint:defer(unknown-a, until: #1)\n\
             // lint:scope-add(unknown-b, exempt_categories=ffi)\n\
             // lint:file-disable(unknown-c) reason: \"x\" tracked: #2\n\
             fn x() {}\n",
        ));
        let mut project = builder.build();
        engine.populate_directives(&mut project).unwrap();
        let err = engine.validate_directives(&project).unwrap_err();
        let directives: Vec<&str> = err
            .iter()
            .map(|e| match e {
                DirectiveValidationError::UnknownLintName { directive, .. } => {
                    *directive
                }
            })
            .collect();
        assert!(directives.contains(&"lint:defer"));
        assert!(directives.contains(&"lint:scope-add"));
        assert!(directives.contains(&"lint:file-disable"));
    }

    #[test]
    fn scope_project_returns_directive_validation_error_for_unknown_lint() {
        // End-to-end integration test: drives the public
        // `scope_project` path so a future regression that removed
        // or reordered the validate_directives call surfaces here.
        // Writes a Rust file to a tempdir, scope_walks it, then
        // asserts the ParseError::DirectiveValidation variant fires.
        use std::fs;

        let tmp = std::env::temp_dir().join("mockspace_validation_gate_e2e");
        let _ = fs::remove_dir_all(&tmp);
        let crate_dir = tmp.join("test_crate").join("src");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(
            tmp.join("test_crate").join("Cargo.toml"),
            "[package]\nname = \"test_crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            crate_dir.join("lib.rs"),
            "// lint:allow(unknown-lint-name) reason: \"x\" tracked: #1\nfn x() {}\n",
        )
        .unwrap();

        let engine = MockspaceEngine::with_entries(vec![stub_lint_entry("known")]);
        let err = engine
            .scope_project(&tmp, RunSurface::Local)
            .expect_err("scope_project should reject project with unknown lint");
        match err {
            ParseError::DirectiveValidation { errors } => {
                assert_eq!(errors.len(), 1);
                let DirectiveValidationError::UnknownLintName { name, .. } =
                    &errors[0];
                assert_eq!(name, "unknown-lint-name");
            }
            other => panic!("expected DirectiveValidation, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn validation_gate_uses_real_directive_span_for_file_disable() {
        // The file-disable diagnostic must point at the comment line,
        // not at a synthesised file:1 placeholder. Previous shape
        // synthesised line=1; the FileDisableEntry now carries a
        // `directive_span`, so this test pins that the diagnostic
        // tracks the real comment location.
        let engine = MockspaceEngine::with_entries(vec![stub_lint_entry("known")]);
        let mut builder =
            ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "c",
            Language::Rust,
            // line 1 is a blank; line 2 has the offending directive.
            "\n// lint:file-disable(unknown-lint) reason: \"x\" tracked: #1\nfn x() {}\n",
        ));
        let mut project = builder.build();
        engine.populate_directives(&mut project).unwrap();
        let err = engine.validate_directives(&project).unwrap_err();
        assert_eq!(err.len(), 1);
        let DirectiveValidationError::UnknownLintName { span, .. } = &err[0];
        // The directive lives on line 2; the prior implementation
        // would have reported line 1.
        assert_eq!(
            span.start_line, 2,
            "expected real comment line, got {}",
            span.start_line
        );
    }

    #[test]
    fn validation_gate_collects_all_errors_at_once() {
        let engine = MockspaceEngine::with_entries(vec![stub_lint_entry("ok")]);
        let mut builder =
            ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a.rs",
            "c",
            Language::Rust,
            "// lint:allow(unk-a) reason: \"x\" tracked: #1\n\
             // lint:allow(unk-b) reason: \"y\" tracked: #2\n\
             // lint:defer(unk-c, until: #3)\n\
             fn x() {}\n",
        ));
        let mut project = builder.build();
        engine.populate_directives(&mut project).unwrap();
        let err = engine.validate_directives(&project).unwrap_err();
        // Three unknown lint names surfaced together rather than
        // one-at-a-time. Confirms the validator continues past the
        // first violation and collects every issue.
        assert_eq!(err.len(), 3);
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
            scope_filter: crate::scope_filter::ScopeFilter::from_config("test", &crate::config_types::ScopeConfig::default()).unwrap(),
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
