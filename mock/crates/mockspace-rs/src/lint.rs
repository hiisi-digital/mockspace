//! The [`Lint`] trait and [`LintMode`] enum.
//!
//! Per schema design memo §1 and §2. Two-method dispatch: [`Lint::check_document`]
//! for `PerDocument` mode, [`Lint::check_project`] for `ProjectScoped` and
//! `TwoPhaseProject` modes. The mode lives on [`crate::CatalogEntry::mode`],
//! not on the trait, so a misconfigured impl can be diagnosed by the engine
//! at first dispatch without silent divergence.

use crate::document::MockspaceDocument;
use crate::errors::LintError;
use crate::finding_sink::FindingSink;
use crate::project::MockspaceProject;
use mockspace_core::lint::{Finding, Fix, GateSeverity, LintContext};

/// The universal lint trait.
///
/// `Send + Sync` so rayon can dispatch per-document parallelism. Concrete
/// lints typically have no interior mutability; any per-invocation state
/// lives inside the `check_*` body, not inside the lint value.
///
/// Each lint registers a [`crate::CatalogEntry`] whose `mode` field selects
/// which method the engine invokes:
///
/// - `LintMode::PerDocument` → `check_document` once per filtered document.
/// - `LintMode::ProjectScoped` or `LintMode::TwoPhaseProject` → `check_project`
///   once per run.
///
/// The default impls of both methods panic with `unreachable!()` if invoked.
/// A misconfigured catalog entry (mode says PerDocument but the impl only
/// has `check_project`) hits the panic on first dispatch with a diagnostic
/// naming the lint. Splitting into two traits was considered; rejected
/// because `Box<dyn Lint>` is the catalog storage shape and a split would
/// force an enum wrapper paying dispatch cost on every call.
pub trait Lint: Send + Sync {
    /// Stable identifier matching the `[lints.<name>]` TOML key.
    fn name(&self) -> &'static str;

    /// One-line human description for diagnostic output.
    fn description(&self) -> &'static str;

    /// Default per-gate severity. Catalog default; consumer TOML overrides.
    fn default_severity(&self) -> GateSeverity;

    /// `PerDocument` mode dispatches here once per filtered document.
    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, doc, sink);
        unreachable!(
            "check_document called on lint `{}` whose mode is not PerDocument; impl mismatch",
            self.name()
        );
    }

    /// `ProjectScoped` and `TwoPhaseProject` modes dispatch here once per run.
    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let _ = (ctx, project, sink);
        unreachable!(
            "check_project called on lint `{}` whose mode is PerDocument; impl mismatch",
            self.name()
        );
    }

    /// Whether this lint needs the `syn` AST cache populated before dispatch.
    /// Engine pre-warms `MockspaceDocument::ast()` across the active document
    /// set when any active lint returns true.
    fn needs_syn_ast(&self) -> bool {
        false
    }

    /// Whether this lint needs the tree-sitter cache populated before dispatch.
    fn needs_tree_sitter(&self) -> bool {
        false
    }

    /// Optional mechanical fix recipe for a finding this lint produced.
    ///
    /// Per the auto-fix design memo at
    /// `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`.
    /// The default impl returns `None`: most lints can hint or describe
    /// the right change but cannot confidently produce a mechanical recipe
    /// because the right substitute depends on call-site semantics the
    /// lint cannot infer.
    ///
    /// Lints that opt in by overriding this method should return `Some(Fix)`
    /// only when the substitution is unambiguously correct on this site.
    /// When in doubt, populate `Finding::suggestion.description` with advice
    /// and leave `Finding::suggestion.fix` (and this method's return) as
    /// `None`. The auto-fix runner applies `Some(Fix)` returns under
    /// `cargo mock check --fix`; `None` returns surface as advice only.
    ///
    /// Byte offsets inside any returned `Fix::Replace`/`Insert`/`Delete`
    /// are UTF-8 indices into the original (pre-strip) source bytes per
    /// the `Fix` type contract.
    ///
    /// The check phase populates `finding.suggestion.fix` from this method
    /// at emit time when the catalog dispatcher invokes it. Lints that
    /// already populate `Finding::suggestion.fix` inline during
    /// `check_document` / `check_project` do not need to override this
    /// method; the engine will not double-call.
    fn fix(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        finding: &Finding,
    ) -> Option<Fix> {
        let _ = (ctx, doc, finding);
        None
    }

    /// Names of `lint:prop` directives this lint reads.
    ///
    /// Per the design memo at
    /// `mock/research/202605220600_lint-provided-marker-directive.md`.
    /// Default empty. Lints that consume prop directives override this
    /// with a static slice listing the names they query against the
    /// resolved [`mockspace_core::lint::PropMap`].
    ///
    /// The future `directive-style-consistency` lint (#548) will use
    /// this method to check that every `lint:prop(name = ...)` in the
    /// project has at least one registered lint that declares it.
    /// Ship the trait method now; the consistency lint that consumes
    /// it ships later as a follow-up. Existing impls do not need to
    /// override this method.
    fn declared_props(&self) -> &'static [&'static str] {
        &[]
    }
}

/// Dispatch mode for a lint, encoded on its [`crate::CatalogEntry::mode`].
///
/// Per schema §2. Read at engine startup; lint dispatch routes accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LintMode {
    /// Engine iterates `project.documents()` (or `staged_documents()` when the
    /// gate's `only_staged = true` and the catalog entry is `staging_aware`),
    /// calls [`Lint::check_document`] for each.
    PerDocument,

    /// Engine calls [`Lint::check_project`] once. The lint reads engine state
    /// (e.g. `SuppressionMap`) but does not iterate documents itself.
    ProjectScoped,

    /// Engine calls [`Lint::check_project`] once. The lint walks
    /// `project.documents()` in Pass 1 (collection) and may walk
    /// `project.staged_documents()` in Pass 2 (validation) per its own logic.
    TwoPhaseProject,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::{Gate, Language, LintCfgStore, RunSurface, Severity, Span};
    use std::borrow::Cow;
    use std::path::Path;

    /// Minimal Lint impl exercising only the required methods. Default
    /// `fix()` should return None without the impl having to spell it out.
    struct MinimalLint;
    impl Lint for MinimalLint {
        fn name(&self) -> &'static str {
            "test-minimal"
        }
        fn description(&self) -> &'static str {
            "minimal lint for trait-default-fix testing"
        }
        fn default_severity(&self) -> GateSeverity {
            GateSeverity::uniform(Severity::Warn)
        }
    }

    /// Lint that opts in to fix() with a deterministic Fix::Replace.
    struct FixableLint;
    impl Lint for FixableLint {
        fn name(&self) -> &'static str {
            "test-fixable"
        }
        fn description(&self) -> &'static str {
            "fixable lint for trait-override-fix testing"
        }
        fn default_severity(&self) -> GateSeverity {
            GateSeverity::uniform(Severity::Error)
        }
        fn fix(
            &self,
            _ctx: &LintContext<'_>,
            _doc: &MockspaceDocument,
            _finding: &Finding,
        ) -> Option<Fix> {
            Some(Fix::Replace {
                start: 10,
                end: 13,
                replacement: Cow::Borrowed("Maybe"),
            })
        }
    }

    fn dummy_finding() -> Finding {
        Finding {
            lint_name: Cow::Borrowed("test"),
            rule_id: None,
            plugin_id: None,
            severity: Severity::Warn,
            impact: None,
            category: None,
            message: Cow::Borrowed("test"),
            span: Span::single_line("test.rs", 1, 1, 1),
            hint: None,
            help: None,
            suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        }
    }

    fn dummy_doc() -> MockspaceDocument {
        MockspaceDocument::new("test.rs", "test-crate", Language::Rust, "fn x() {}")
    }

    struct EmptyCfg;
    impl LintCfgStore for EmptyCfg {
        fn get(&self, _lint_name: &str) -> Option<&toml::Table> {
            None
        }
    }

    #[test]
    fn default_declared_props_is_empty() {
        let lint = MinimalLint;
        assert!(lint.declared_props().is_empty());
    }

    #[test]
    fn overridden_declared_props_returns_static_slice() {
        struct WithProps;
        impl Lint for WithProps {
            fn name(&self) -> &'static str {
                "test-props"
            }
            fn description(&self) -> &'static str {
                "lint that declares props it reads"
            }
            fn default_severity(&self) -> GateSeverity {
                GateSeverity::uniform(Severity::Warn)
            }
            fn declared_props(&self) -> &'static [&'static str] {
                &["audited", "arena_size", "thread_safe"]
            }
        }
        let lint = WithProps;
        let props = lint.declared_props();
        assert_eq!(props.len(), 3);
        assert!(props.contains(&"audited"));
        assert!(props.contains(&"arena_size"));
        assert!(props.contains(&"thread_safe"));
    }

    #[test]
    fn default_fix_returns_none() {
        let lint = MinimalLint;
        let cfg = EmptyCfg;
        let root = Path::new("/tmp");
        let ctx = LintContext {
            gate: Gate::Commit,
            severities: GateSeverity::uniform(Severity::Warn),
            surface: RunSurface::Local,
            project_root: root,
            config: &cfg,
        };
        let doc = dummy_doc();
        let finding = dummy_finding();
        assert!(lint.fix(&ctx, &doc, &finding).is_none());
    }

    #[test]
    fn overridden_fix_returns_some_fix() {
        let lint = FixableLint;
        let cfg = EmptyCfg;
        let root = Path::new("/tmp");
        let ctx = LintContext {
            gate: Gate::Commit,
            severities: GateSeverity::uniform(Severity::Error),
            surface: RunSurface::Local,
            project_root: root,
            config: &cfg,
        };
        let doc = dummy_doc();
        let finding = dummy_finding();
        let fix = lint
            .fix(&ctx, &doc, &finding)
            .expect("fixable lint returns Some");
        match fix {
            Fix::Replace {
                start,
                end,
                replacement,
            } => {
                assert_eq!(start, 10);
                assert_eq!(end, 13);
                assert_eq!(replacement.as_ref(), "Maybe");
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }
}
