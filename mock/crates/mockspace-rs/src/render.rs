//! Render pipeline orchestrator for `mock/*.md.tmpl` documentation.
//!
//! Walks the three mock-root templates (`DESIGN.md.tmpl`,
//! `PRINCIPLES.md.tmpl`, `WORKFLOW.md.tmpl`), renders each through
//! [`mockspace_template`], and writes the output to `out_dir/<name>.md`
//! via [`mockspace_template::render_atomic`].
//!
//! The public surface is two functions ([`regenerate`] and [`check`])
//! reused across the broader render-pipeline slice plan; new behaviour
//! is added behind private helpers without breaking the consumer
//! surface. Today this module covers the mock-root files only;
//! per-crate output and dependency-graph rendering follow as additive
//! extensions of the same two entry points.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use mockspace_template::{render_atomic, RenderError, Template, TemplateEnv};

use crate::project::MockspaceProject;

/// Mock-root templates rendered by slice 2. Order is the on-disk write
/// order; output filenames drop the trailing `.tmpl`.
const MOCK_ROOT_TEMPLATES: &[&str] = &[
    "DESIGN.md.tmpl",
    "PRINCIPLES.md.tmpl",
    "WORKFLOW.md.tmpl",
];

/// Outcome of a successful [`regenerate`] pass.
#[derive(Debug, Default)]
pub struct RegenerateReport {
    /// Per-file outcomes, in template-walk order.
    pub files: Vec<RenderedFile>,
}

/// Outcome for a single rendered file.
#[derive(Debug)]
pub struct RenderedFile {
    pub path: PathBuf,
    pub state: WriteState,
}

/// How the atomic write resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteState {
    /// Destination did not exist; new file created.
    Created,
    /// Destination existed with different bytes; rewritten.
    Updated,
    /// Destination existed with byte-identical content; write skipped
    /// and the destination's mtime is preserved.
    Unchanged,
}

/// Outcome of a [`check`] (`regenerate --check`) pass.
///
/// `has_drift()` is the canonical exit-code signal: CI consumers fail on
/// any non-empty `drifted` or `missing` list.
#[derive(Debug, Default)]
pub struct CheckReport {
    /// Files where rendered bytes differ from what is on disk.
    pub drifted: Vec<PathBuf>,
    /// Files where rendered bytes match what is on disk.
    pub matched: Vec<PathBuf>,
    /// Files the renderer would create (nothing on disk to compare).
    pub missing: Vec<PathBuf>,
}

impl CheckReport {
    /// True if any file would change on a real regenerate pass.
    ///
    /// Collapses [`drifted`](Self::drifted) and [`missing`](Self::missing)
    /// into one boolean. CI consumers fail when this returns true.
    pub fn needs_regen(&self) -> bool {
        !self.drifted.is_empty() || !self.missing.is_empty()
    }
}

/// Things that can go wrong during render orchestration.
#[derive(Debug)]
pub enum RegenerateError {
    /// Filesystem operation failed. `path` is the file or directory the
    /// operation targeted; `source` is the underlying io error.
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// Expected template missing at the mock root.
    TemplateMissing(PathBuf),
    /// Template engine surfaced a render error (undefined variable,
    /// syntax error, etc.).
    Render(RenderError),
}

impl RegenerateError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }
}

impl From<RenderError> for RegenerateError {
    fn from(e: RenderError) -> Self {
        Self::Render(e)
    }
}

impl std::fmt::Display for RegenerateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "io error on {}: {source}", path.display())
            }
            Self::TemplateMissing(p) => write!(f, "template missing: {}", p.display()),
            Self::Render(e) => write!(f, "render error: {e}"),
        }
    }
}

impl std::error::Error for RegenerateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TemplateMissing(_) => None,
            Self::Render(e) => Some(e),
        }
    }
}

/// Render the mock-root templates and write them to `out_dir`.
///
/// Reads `<project.root()>/mock/{DESIGN,PRINCIPLES,WORKFLOW}.md.tmpl`,
/// renders each with a context built from the project (workspace member
/// summaries, plus a `dep_graph` placeholder that slice 4 will fill),
/// and writes the result atomically to `out_dir/<name>.md`. The output
/// directory is created if absent.
pub fn regenerate(
    project: &MockspaceProject,
    out_dir: &Path,
) -> Result<RegenerateReport, RegenerateError> {
    let mock_root = project.root().join("mock");
    let context = build_context(project)?;
    let env = build_env(&mock_root)?;

    fs::create_dir_all(out_dir)
        .map_err(|e| RegenerateError::io(out_dir, e))?;

    let mut files = Vec::with_capacity(MOCK_ROOT_TEMPLATES.len());
    for tmpl_name in MOCK_ROOT_TEMPLATES {
        let template = env.get_template(tmpl_name)?;
        let output_name = strip_tmpl_suffix(tmpl_name);
        let dest = out_dir.join(output_name);

        let state = write_with_state(&template, &context, &dest)?;
        files.push(RenderedFile { path: dest, state });
    }

    Ok(RegenerateReport { files })
}

/// Render the mock-root templates in memory and compare against the
/// existing files in `out_dir`.
///
/// Read-only; never writes. Suitable for CI gating: pair with
/// [`CheckReport::has_drift`] to fail builds when the rendered output
/// diverges from the committed copy.
pub fn check(
    project: &MockspaceProject,
    out_dir: &Path,
) -> Result<CheckReport, RegenerateError> {
    let mock_root = project.root().join("mock");
    let context = build_context(project)?;
    let env = build_env(&mock_root)?;

    let mut report = CheckReport::default();
    for tmpl_name in MOCK_ROOT_TEMPLATES {
        let template = env.get_template(tmpl_name)?;
        let rendered = template.render(&context)?;
        let output_name = strip_tmpl_suffix(tmpl_name);
        let dest = out_dir.join(output_name);

        match fs::read_to_string(&dest) {
            Ok(existing) if existing == rendered => report.matched.push(dest),
            Ok(_) => report.drifted.push(dest),
            Err(e) if e.kind() == io::ErrorKind::NotFound => report.missing.push(dest),
            Err(e) => return Err(RegenerateError::io(dest, e)),
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// Helpers (internal; not part of the public surface).

/// Context handed to mock-root templates.
///
/// Slice 2 ships `crates` (per-workspace-member summaries from each
/// crate's `README.md.tmpl`) and a `dep_graph` placeholder. Slice 4 will
/// populate `dep_graph` with the rendered Graphviz output.
#[derive(Serialize, Default)]
struct RenderContext {
    crates: Vec<CrateSummary>,
    dep_graph: String,
}

#[derive(Serialize)]
struct CrateSummary {
    name: String,
    /// Raw body of `mock/crates/<name>/README.md.tmpl`, or an empty
    /// string if the file is absent. Future work will replace this with
    /// the rendered first-paragraph summary; today the raw text is
    /// surfaced so the template can decide what to do with it.
    body: String,
}

fn build_context(project: &MockspaceProject) -> Result<RenderContext, RegenerateError> {
    let mock_root = project.root().join("mock");
    // Alphabetical ordering on crate name, not the crate_graph's
    // construction order. The graph's order today comes from
    // cargo-metadata's emit sequence, which is stable but not
    // semantically meaningful and could shift across graph-builder
    // refactors. Alphabetical keeps `docs/DESIGN.md` git-stable across
    // those internal changes.
    let mut crates: BTreeMap<String, String> = BTreeMap::new();

    for info in &project.crate_graph().crates {
        if !info.is_workspace_member {
            continue;
        }
        let readme = mock_root
            .join("crates")
            .join(&info.name)
            .join("README.md.tmpl");

        let body = match fs::read_to_string(&readme) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(RegenerateError::io(readme, e)),
        };

        crates.insert(info.name.clone(), body);
    }

    let crates = crates
        .into_iter()
        .map(|(name, body)| CrateSummary { name, body })
        .collect();

    Ok(RenderContext {
        crates,
        dep_graph: String::new(),
    })
}

fn build_env(mock_root: &Path) -> Result<TemplateEnv, RegenerateError> {
    let mut env = TemplateEnv::new();
    for tmpl_name in MOCK_ROOT_TEMPLATES {
        let src = mock_root.join(tmpl_name);
        let body = match fs::read_to_string(&src) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(RegenerateError::TemplateMissing(src));
            }
            Err(e) => return Err(RegenerateError::io(src, e)),
        };
        env.add_template(tmpl_name, &body)?;
    }
    Ok(env)
}

fn strip_tmpl_suffix(name: &str) -> &str {
    name.strip_suffix(".tmpl").unwrap_or(name)
}

/// Atomic-write the rendered template, classifying the outcome.
///
/// Creates the destination's parent directory if absent so callers
/// writing into nested subdirs (e.g. `out_dir/<crate>/DESIGN.md`) need
/// not pre-create per-subdir.
///
/// Classification happens by reading the destination once before
/// delegating to `render_atomic`. A non-NotFound read error short-
/// circuits with `Io` rather than silently rewriting.
///
/// Note: `render_atomic` performs its own byte-compare for the
/// idempotency skip, so an Unchanged outcome reads the destination
/// twice. With three mock-root files the cost is negligible; when
/// per-crate fan-out scales this to N files, fold the classification
/// into `mockspace-template` by exposing a `render_atomic_classified`
/// variant that returns `WriteState` from the single internal compare.
fn write_with_state<C: Serialize>(
    template: &Template<'_>,
    context: &C,
    dest: &Path,
) -> Result<WriteState, RegenerateError> {
    let rendered = template.render(context)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| RegenerateError::io(parent, e))?;
    }

    let state = match fs::read_to_string(dest) {
        Ok(existing) if existing == rendered => return Ok(WriteState::Unchanged),
        Ok(_) => WriteState::Updated,
        Err(e) if e.kind() == io::ErrorKind::NotFound => WriteState::Created,
        Err(e) => return Err(RegenerateError::io(dest, e)),
    };

    render_atomic(template, context, dest)?;
    Ok(state)
}

// ---------------------------------------------------------------------------
// Tests.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{CrateGraph, CrateInfo, ProjectBuilder};
    use mockspace_core::lint::{Gate, RunSurface};
    use std::fs;
    use tempfile::TempDir;

    /// Build a project rooted at `root` with the given workspace members
    /// (each represented as a crate name; pass empty slice for none).
    fn project(root: &Path, members: &[&str]) -> MockspaceProject {
        let crates = members
            .iter()
            .map(|name| CrateInfo {
                name: (*name).to_string(),
                root_path: root.join(name),
                is_proc_macro: false,
                is_workspace_member: true,
                deps: Vec::new(),
            })
            .collect::<Vec<_>>();

        let mut by_name = std::collections::HashMap::new();
        for (i, c) in crates.iter().enumerate() {
            by_name.insert(c.name.clone(), i);
        }
        let graph = CrateGraph { crates, by_name };

        ProjectBuilder::new(root.to_path_buf(), RunSurface::Ci, Gate::Commit)
            .with_crate_graph(graph)
            .build()
    }

    /// Write `mock/{DESIGN,PRINCIPLES,WORKFLOW}.md.tmpl` under `root`
    /// with simple `{{crates}}`-aware bodies that the tests can verify.
    fn write_mock_root_templates(root: &Path) {
        let mock = root.join("mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(
            mock.join("DESIGN.md.tmpl"),
            "design\n{% for c in crates %}- {{ c.name }}\n{% endfor %}",
        )
        .unwrap();
        fs::write(mock.join("PRINCIPLES.md.tmpl"), "principles only").unwrap();
        fs::write(mock.join("WORKFLOW.md.tmpl"), "workflow only").unwrap();
    }

    #[test]
    fn regenerate_writes_three_mock_root_files() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        let report = regenerate(&proj, &out).expect("regenerate");

        assert_eq!(report.files.len(), 3);
        assert!(out.join("DESIGN.md").is_file());
        assert!(out.join("PRINCIPLES.md").is_file());
        assert!(out.join("WORKFLOW.md").is_file());
        for f in &report.files {
            assert_eq!(f.state, WriteState::Created);
        }
    }

    #[test]
    fn regenerate_interpolates_crate_summaries() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &["alpha", "beta"]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let design = fs::read_to_string(out.join("DESIGN.md")).unwrap();
        assert!(design.contains("- alpha\n"));
        assert!(design.contains("- beta\n"));
    }

    #[test]
    fn regenerate_is_idempotent_on_second_pass() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("first");
        let report = regenerate(&proj, &out).expect("second");

        for f in &report.files {
            assert_eq!(
                f.state,
                WriteState::Unchanged,
                "expected Unchanged on idempotent rerender, got {:?} for {}",
                f.state,
                f.path.display()
            );
        }
    }

    #[test]
    fn regenerate_marks_updated_when_template_changes() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("first");

        // Mutate one template body. Second pass should classify it
        // as Updated, the others as Unchanged.
        fs::write(
            tmp.path().join("mock").join("PRINCIPLES.md.tmpl"),
            "principles v2",
        )
        .unwrap();
        let report = regenerate(&proj, &out).expect("second");

        let principles = report
            .files
            .iter()
            .find(|f| f.path.ends_with("PRINCIPLES.md"))
            .unwrap();
        assert_eq!(principles.state, WriteState::Updated);
    }

    #[test]
    fn regenerate_errors_when_template_missing() {
        let tmp = TempDir::new().unwrap();
        // Intentionally create only two of the three.
        let mock = tmp.path().join("mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(mock.join("DESIGN.md.tmpl"), "ok").unwrap();
        fs::write(mock.join("PRINCIPLES.md.tmpl"), "ok").unwrap();
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        let err = regenerate(&proj, &out).expect_err("should fail");
        match err {
            RegenerateError::TemplateMissing(p) => {
                assert!(p.ends_with("WORKFLOW.md.tmpl"), "got {}", p.display());
            }
            other => panic!("expected TemplateMissing, got {other:?}"),
        }
    }

    #[test]
    fn check_reports_matched_when_disk_equals_render() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("seed");
        let report = check(&proj, &out).expect("check");

        assert_eq!(report.matched.len(), 3);
        assert!(report.drifted.is_empty());
        assert!(report.missing.is_empty());
        assert!(!report.needs_regen());
    }

    #[test]
    fn check_reports_drift_when_disk_diverges() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("seed");
        // Hand-edit the rendered output to simulate stale committed copy.
        fs::write(out.join("PRINCIPLES.md"), "hand-edited").unwrap();

        let report = check(&proj, &out).expect("check");
        assert_eq!(report.drifted.len(), 1);
        assert!(report.drifted[0].ends_with("PRINCIPLES.md"));
        assert_eq!(report.matched.len(), 2);
        assert!(report.needs_regen());
    }

    #[test]
    fn check_reports_missing_when_output_not_yet_written() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");
        // Note: no regenerate first, so out_dir doesn't even exist.

        let report = check(&proj, &out).expect("check");
        assert_eq!(report.missing.len(), 3);
        assert!(report.matched.is_empty());
        assert!(report.drifted.is_empty());
        assert!(report.needs_regen());
    }

    #[test]
    fn regenerate_handles_existing_empty_out_dir() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");
        // Pre-create the destination directory empty. This locks the
        // "out_dir exists, no children" path against a future refactor
        // that swaps `create_dir_all` for `create_dir`.
        fs::create_dir(&out).unwrap();

        let report = regenerate(&proj, &out).expect("regenerate");
        assert_eq!(report.files.len(), 3);
        for f in &report.files {
            assert_eq!(f.state, WriteState::Created);
        }
    }

    #[test]
    fn build_context_reads_per_crate_readme_when_present() {
        let tmp = TempDir::new().unwrap();
        let crate_dir = tmp.path().join("mock").join("crates").join("alpha");
        fs::create_dir_all(&crate_dir).unwrap();
        fs::write(crate_dir.join("README.md.tmpl"), "alpha summary").unwrap();
        let proj = project(tmp.path(), &["alpha"]);

        let ctx = build_context(&proj).unwrap();
        assert_eq!(ctx.crates.len(), 1);
        assert_eq!(ctx.crates[0].name, "alpha");
        assert_eq!(ctx.crates[0].body, "alpha summary");
    }

    #[test]
    fn build_context_tolerates_missing_per_crate_readme() {
        let tmp = TempDir::new().unwrap();
        let proj = project(tmp.path(), &["alpha", "beta"]);

        let ctx = build_context(&proj).unwrap();
        assert_eq!(ctx.crates.len(), 2);
        assert!(ctx.crates.iter().all(|c| c.body.is_empty()));
    }
}
