//! [`MockspaceProject`] concrete type per schema design memo §8.
//!
//! Holds the document set, staged-index subset, crate graph, workspace
//! metadata, parsed design rounds, suppression map, and the current
//! `RunSurface` and `Gate`.
//!
//! Built once per run by [`crate::MockspaceEngine::scope_project`] and
//! handed to project-mode lints. PerDocument lints see individual
//! [`MockspaceDocument`]s out of this collection.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mockspace_core::lint::{
    FileDisableSet, Gate, IntroducerMap, Project, RunSurface, ScopeAddMap, SuppressionMap,
};

use crate::document::MockspaceDocument;

/// The concrete project handed to project-mode lints.
pub struct MockspaceProject {
    pub(crate) root: PathBuf,
    pub(crate) documents: Vec<MockspaceDocument>,
    pub(crate) staged_indices: Vec<usize>,
    pub(crate) crate_graph: CrateGraph,
    pub(crate) workspace: WorkspaceMetadata,
    pub(crate) design_rounds: DesignRoundsView,
    pub(crate) suppressions: SuppressionMap,
    pub(crate) introducers: IntroducerMap,
    pub(crate) scope_adds: ScopeAddMap,
    pub(crate) file_disables: FileDisableSet,
    pub(crate) surface: RunSurface,
    pub(crate) gate: Gate,
    pub(crate) introduced_categories: HashMap<String, HashSet<String>>,
}

impl std::fmt::Debug for MockspaceProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockspaceProject")
            .field("root", &self.root)
            .field("documents", &self.documents.len())
            .field("staged", &self.staged_indices.len())
            .field("crates", &self.crate_graph.crates.len())
            .field("design_rounds", &self.design_rounds.rounds.len())
            .field("surface", &self.surface)
            .field("gate", &self.gate)
            .finish()
    }
}

impl MockspaceProject {
    /// Iterate every document.
    pub fn documents(&self) -> impl Iterator<Item = &MockspaceDocument> {
        self.documents.iter()
    }

    /// Iterate only documents that count as "staged" per the current gate.
    ///
    /// In [`RunSurface::Editor`], every loaded document counts as staged so
    /// PerDocument lints with `only_staged = true` actually see the buffer
    /// the editor is asking about.
    pub fn staged_documents(&self) -> impl Iterator<Item = &MockspaceDocument> {
        self.staged_indices.iter().map(move |&i| &self.documents[i])
    }

    pub fn document_at(&self, index: usize) -> Option<&MockspaceDocument> {
        self.documents.get(index)
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn crate_graph(&self) -> &CrateGraph {
        &self.crate_graph
    }

    pub fn workspace(&self) -> &WorkspaceMetadata {
        &self.workspace
    }

    pub fn design_rounds(&self) -> &DesignRoundsView {
        &self.design_rounds
    }

    pub fn suppressions(&self) -> &SuppressionMap {
        &self.suppressions
    }

    /// Aggregated `lint:introduces` records from every Rust document
    /// in this project. Populated at `scope_project` time by the
    /// engine's preprocessor pass; consumer lints read this to
    /// decide their own carve-outs.
    pub fn introducers(&self) -> &IntroducerMap {
        &self.introducers
    }

    /// Aggregated `lint:scope-add` records. Lints that consult this
    /// extend their pre-compiled `ScopeFilter` per-finding.
    pub fn scope_adds(&self) -> &ScopeAddMap {
        &self.scope_adds
    }

    /// Aggregated `lint:file-disable` records. The engine consults
    /// this before suppression resolution to drop findings whose
    /// `(file, lint)` pair is disabled outright.
    pub fn file_disables(&self) -> &FileDisableSet {
        &self.file_disables
    }

    /// Replace the project's resolved-directive state. Called by the
    /// engine after `scope_walk` completes; consumers should not call
    /// this directly.
    pub fn set_resolved_directives(
        &mut self,
        suppressions: SuppressionMap,
        introducers: IntroducerMap,
        scope_adds: ScopeAddMap,
        file_disables: FileDisableSet,
    ) {
        self.suppressions = suppressions;
        self.introducers = introducers;
        self.scope_adds = scope_adds;
        self.file_disables = file_disables;
    }

    pub fn surface(&self) -> RunSurface {
        self.surface
    }

    pub fn gate(&self) -> Gate {
        self.gate
    }

    /// Categories declared in `[primitive-introductions]` for the named
    /// crate. Returns an empty set if the crate did not declare any.
    pub fn introduced_categories(&self, crate_name: &str) -> Option<&HashSet<String>> {
        self.introduced_categories.get(crate_name)
    }
}

impl Project for MockspaceProject {
    fn root(&self) -> &Path {
        &self.root
    }
    fn surface(&self) -> RunSurface {
        self.surface
    }
    // Inherit the default Project::documents() returning &[].
    // Engine-internal callers use the typed iterator at
    // [`MockspaceProject::documents`] (the inherent method on the
    // concrete type, not the trait method) for full access.
}

// =========================================================================
// CrateGraph.
// =========================================================================

/// Built once at project load from `cargo metadata` (or equivalent
/// inspection of the workspace `Cargo.toml`).
#[derive(Debug, Clone, Default)]
pub struct CrateGraph {
    pub crates: Vec<CrateInfo>,
    pub by_name: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub root_path: PathBuf,
    pub is_proc_macro: bool,
    pub is_workspace_member: bool,
    pub deps: Vec<String>,
}

impl CrateGraph {
    pub fn get(&self, name: &str) -> Option<&CrateInfo> {
        self.by_name.get(name).map(|&i| &self.crates[i])
    }

    pub fn is_proc_macro(&self, name: &str) -> bool {
        self.get(name).map_or(false, |c| c.is_proc_macro)
    }
}

// =========================================================================
// WorkspaceMetadata.
// =========================================================================

#[derive(Debug, Clone, Default)]
pub struct WorkspaceMetadata {
    pub root: PathBuf,
    pub proc_macro_crates: HashSet<String>,
    pub task_state: TaskStateView,
}

/// Engine-visible view of the workspace task tracker. Empty by default;
/// populated by the engine when the consumer's mock/tasks tree is loaded.
#[derive(Debug, Clone, Default)]
pub struct TaskStateView {
    pub open_tasks: HashSet<String>,
    pub closed_tasks: HashSet<String>,
}

impl TaskStateView {
    pub fn is_open(&self, task_ref: &str) -> bool {
        self.open_tasks.contains(task_ref)
    }
    pub fn is_closed(&self, task_ref: &str) -> bool {
        self.closed_tasks.contains(task_ref)
    }
    pub fn is_known(&self, task_ref: &str) -> bool {
        self.is_open(task_ref) || self.is_closed(task_ref)
    }
}

// =========================================================================
// DesignRoundsView.
// =========================================================================

/// Engine-visible snapshot of design-round artefacts on disk. Built once
/// at project load by walking `mock/design_rounds/` and parsing each
/// round's `round.toml` (when present).
#[derive(Debug, Clone, Default)]
pub struct DesignRoundsView {
    pub root: PathBuf,
    pub rounds: Vec<DesignRound>,
}

#[derive(Debug, Clone)]
pub struct DesignRound {
    pub timestamp: String,
    pub state: RoundState,
    pub doc_cl: Option<PathBuf>,
    pub src_cl: Option<PathBuf>,
    pub locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoundState {
    Topic,
    Doc,
    Src,
    Locked,
    Closed,
}

// =========================================================================
// Project builder.
// =========================================================================

/// In-memory project builder. Today used by tests; the production
/// `scope_project` walker uses the same set of setters.
pub struct ProjectBuilder {
    root: PathBuf,
    documents: Vec<MockspaceDocument>,
    staged_indices: Vec<usize>,
    crate_graph: CrateGraph,
    workspace: WorkspaceMetadata,
    design_rounds: DesignRoundsView,
    suppressions: SuppressionMap,
    surface: RunSurface,
    gate: Gate,
    introduced_categories: HashMap<String, HashSet<String>>,
}

impl ProjectBuilder {
    pub fn new(root: impl Into<PathBuf>, surface: RunSurface, gate: Gate) -> Self {
        let root = root.into();
        Self {
            workspace: WorkspaceMetadata {
                root: root.clone(),
                ..Default::default()
            },
            root,
            documents: Vec::new(),
            staged_indices: Vec::new(),
            crate_graph: CrateGraph::default(),
            design_rounds: DesignRoundsView::default(),
            suppressions: SuppressionMap::new(),
            surface,
            gate,
            introduced_categories: HashMap::new(),
        }
    }

    pub fn push_document(&mut self, doc: MockspaceDocument) -> usize {
        let idx = self.documents.len();
        self.documents.push(doc);
        idx
    }

    /// Mark a previously pushed document index as staged.
    pub fn mark_staged(&mut self, index: usize) {
        if !self.staged_indices.contains(&index) {
            self.staged_indices.push(index);
        }
    }

    /// Mark every document staged. Used in [`RunSurface::Editor`] and as
    /// the staging-filter `Full` fallback.
    pub fn mark_all_staged(&mut self) {
        self.staged_indices = (0..self.documents.len()).collect();
    }

    pub fn with_crate_graph(mut self, graph: CrateGraph) -> Self {
        self.crate_graph = graph;
        self
    }

    pub fn with_workspace(mut self, ws: WorkspaceMetadata) -> Self {
        self.workspace = ws;
        self
    }

    /// Focused setter that only updates `workspace.proc_macro_crates`,
    /// preserving any other workspace state the builder has accumulated.
    /// Prefer this over [`with_workspace`] when only the proc-macro set
    /// is changing.
    pub fn with_proc_macro_crates(mut self, set: HashSet<String>) -> Self {
        self.workspace.proc_macro_crates = set;
        self
    }

    pub fn with_suppressions(mut self, sup: SuppressionMap) -> Self {
        self.suppressions = sup;
        self
    }

    pub fn with_design_rounds(mut self, rounds: DesignRoundsView) -> Self {
        self.design_rounds = rounds;
        self
    }

    pub fn declare_introduced_category(
        &mut self,
        crate_name: impl Into<String>,
        category: impl Into<String>,
    ) {
        self.introduced_categories
            .entry(crate_name.into())
            .or_default()
            .insert(category.into());
    }

    pub fn build(self) -> MockspaceProject {
        MockspaceProject {
            root: self.root,
            documents: self.documents,
            staged_indices: self.staged_indices,
            crate_graph: self.crate_graph,
            workspace: self.workspace,
            design_rounds: self.design_rounds,
            suppressions: self.suppressions,
            introducers: IntroducerMap::new(),
            scope_adds: ScopeAddMap::new(),
            file_disables: FileDisableSet::new(),
            surface: self.surface,
            gate: self.gate,
            introduced_categories: self.introduced_categories,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::Language;

    #[test]
    fn builder_assembles_project() {
        let mut b = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        let _i = b.push_document(MockspaceDocument::new(
            "a.rs",
            "my-crate",
            Language::Rust,
            "fn x() {}",
        ));
        let project = b.build();
        assert_eq!(project.document_count(), 1);
        assert_eq!(project.gate(), Gate::Commit);
    }

    #[test]
    fn editor_marks_all_staged() {
        let mut b = ProjectBuilder::new("/tmp", RunSurface::Editor, Gate::Commit);
        b.push_document(MockspaceDocument::new(
            "a.rs",
            "my-crate",
            Language::Rust,
            "x",
        ));
        b.push_document(MockspaceDocument::new(
            "b.rs",
            "my-crate",
            Language::Rust,
            "y",
        ));
        b.mark_all_staged();
        let project = b.build();
        assert_eq!(project.staged_documents().count(), 2);
    }
}
