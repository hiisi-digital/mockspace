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
use mockspace_core::lint::{GateSeverity, LintContext};

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
