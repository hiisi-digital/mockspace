//! Render pipeline for `mock/*.md.tmpl` documentation.
//!
//! Walks two surfaces:
//!
//! 1. The mock-root templates (`DESIGN.md.tmpl`, `PRINCIPLES.md.tmpl`,
//!    `WORKFLOW.md.tmpl`), renders each with a workspace-wide context,
//!    and writes to `out_dir/<name>.md`.
//! 2. Per workspace-member crate: any of
//!    `mock/crates/<name>/{DESIGN,BACKLOG}.md.tmpl` and every
//!    `mock/crates/<name>/deepdives/*.md.tmpl` present on disk. Each
//!    renders with a per-crate context (name, deps, `is_proc_macro`)
//!    and writes to `out_dir/<name>/<file>.md`. Optional templates
//!    silently skip when absent; `SHAME.md.tmpl` is never read.
//!
//! Both surfaces flow through [`mockspace_template::render_atomic`]
//! for the write step.
//!
//! A third output, `<out_dir>/dep-graph.dot`, captures the
//! workspace's crate dependency graph as Graphviz source. When the
//! system `dot` binary is on PATH a sibling `<out_dir>/dep-graph.svg`
//! lands too; absent Graphviz only the `.dot` is produced. The same
//! source string is exposed to templates via `{{ dep_graph }}`.
//!
//! Public surface: two functions ([`regenerate`] and [`check`]) reused
//! across the broader render-pipeline slice plan. New behaviour lands
//! behind private helpers without breaking the consumer surface.

pub mod dep_graph;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use mockspace_template::{render_atomic, write_atomic, RenderError, Template, TemplateEnv};

use crate::project::{CrateInfo, MockspaceProject};

/// Mock-root templates. Order is the on-disk write order; output
/// filenames drop the trailing `.tmpl`.
const MOCK_ROOT_TEMPLATES: &[&str] = &[
    "DESIGN.md.tmpl",
    "PRINCIPLES.md.tmpl",
    "WORKFLOW.md.tmpl",
];

/// Per-crate "leaf" templates that ship to public docs. Each is
/// optional: a crate without one of these silently skips it.
///
/// `SHAME.md.tmpl` is intentionally absent. The lint engine reads it
/// as workspace-internal accounting; it must not land in the rendered
/// public docs tree.
const CRATE_LEAF_TEMPLATES: &[&str] = &["DESIGN.md.tmpl", "BACKLOG.md.tmpl"];

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

/// Things that can go wrong while rendering.
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

/// Render the mock-root templates and the per-crate templates, writing
/// the result tree to `out_dir`.
///
/// Mock-root: `<project.root()>/mock/{DESIGN,PRINCIPLES,WORKFLOW}.md.tmpl`
/// → `out_dir/<name>.md`.
///
/// Per workspace-member crate: any of `mock/crates/<name>/{DESIGN,BACKLOG}.md.tmpl`
/// and every `mock/crates/<name>/deepdives/*.md.tmpl` that exists. Each
/// renders to `out_dir/<name>/<file>.md`. Templates that are absent
/// silently skip; `SHAME.md.tmpl` is never read.
///
/// Dependency-graph: `out_dir/dep-graph.dot` always lands; when the
/// system `dot` binary is on PATH the rendered SVG lands beside it as
/// `out_dir/dep-graph.svg`. Both appear in the returned
/// [`RegenerateReport`].
///
/// The output root directory is created if absent; per-crate subdirs
/// are auto-created by the atomic-write helper.
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

    regenerate_per_crate(project, out_dir, &mut files)?;
    write_dep_graph(&context.dep_graph, out_dir, &mut files)?;
    regenerate_benches(project, out_dir, &mut files)?;

    Ok(RegenerateReport { files })
}

/// Output filename for the Graphviz dependency-graph `.dot` source.
const DEP_GRAPH_DOT: &str = "dep-graph.dot";

/// Output filename for the rendered SVG; produced only when the
/// system `dot` binary is on PATH.
const DEP_GRAPH_SVG: &str = "dep-graph.svg";

/// Write the dep-graph `.dot` source to `out_dir/dep-graph.dot`
/// atomically. When the system `dot` binary succeeds at rendering
/// the source to SVG, also writes `out_dir/dep-graph.svg`. Both
/// outputs are appended to `out` so the report reflects what landed.
///
/// SVG output is best-effort. Graphviz absent on PATH, spawn
/// failure, or non-zero exit all result in no SVG file and no
/// report entry. The `.dot` file is always written; templates can
/// reference it directly for downstream consumers who need the
/// source rather than the rendered image.
fn write_dep_graph(
    dot_source: &str,
    out_dir: &Path,
    out: &mut Vec<RenderedFile>,
) -> Result<(), RegenerateError> {
    let dot_dest = out_dir.join(DEP_GRAPH_DOT);
    let dot_state = write_raw_with_state(dot_source, &dot_dest)?;
    out.push(RenderedFile { path: dot_dest, state: dot_state });

    if let Some(svg) = dep_graph::try_render_svg(dot_source) {
        let svg_dest = out_dir.join(DEP_GRAPH_SVG);
        let svg_state = write_raw_with_state(&svg, &svg_dest)?;
        out.push(RenderedFile { path: svg_dest, state: svg_state });
    }

    Ok(())
}

/// Classify the dep-graph `.dot` output against on-disk content,
/// appending to `report`. The SVG is best-effort and skipped here:
/// it depends on Graphviz being on PATH at check time, and CI
/// environments that lack the binary should not fail check on
/// `.svg` drift.
fn check_dep_graph(
    dot_source: &str,
    out_dir: &Path,
    report: &mut CheckReport,
) -> Result<(), RegenerateError> {
    let dest = out_dir.join(DEP_GRAPH_DOT);
    classify_against_disk(&dest, dot_source, report)
}

/// Subdirectory under `<mock_root>/` that holds bench bundles.
const BENCHES_DIR: &str = "benches";

/// Suffix that identifies a multi-size bench-findings filename.
/// A file `<size>_findings.md` strips this suffix to yield the
/// `<size>` token used as the rendered filename.
const SIZE_FINDINGS_SUFFIX: &str = "_findings.md";

/// Walk each bench bundle under `<project.root()>/mock/benches/<bundle>/`
/// and copy `findings.md` files into the rendered docs tree.
///
/// Per the slice plan at
/// `mock/research/202605221502_phase-3-bench-sketch-render-integration.md`,
/// two modes are recognised per bundle:
///
/// - **Single-size**: the bundle contains exactly one `findings.md`
///   at its root. Rendered to `out_dir/benches/<bundle>.md`.
/// - **Multi-size**: the bundle contains one or more
///   `<size>_findings.md` files (no top-level `findings.md`).
///   Each renders to `out_dir/benches/<bundle>/<size>.md`.
///
/// If a bundle has both shapes simultaneously, single-size wins;
/// the per-size files are silently dropped. This is a deliberate
/// precedence call rather than an error: the spec carries no
/// invariant excluding the mixed case, and a warning channel
/// does not exist on `RegenerateReport` today.
///
/// Bundles missing both shapes are silently skipped (mid-authoring
/// bundles do not block render). The `mock/benches/` directory
/// missing entirely is also silently skipped.
fn regenerate_benches(
    project: &MockspaceProject,
    out_dir: &Path,
    out: &mut Vec<RenderedFile>,
) -> Result<(), RegenerateError> {
    let benches_root = project.root().join("mock").join(BENCHES_DIR);
    let bundles = match collect_bench_bundles(&benches_root)? {
        Some(b) => b,
        None => return Ok(()),
    };

    for bundle in bundles {
        for output in bundle.outputs {
            let dest = out_dir.join(BENCHES_DIR).join(&output.relative_dest);
            let body = fs::read_to_string(&output.source)
                .map_err(|e| RegenerateError::io(output.source.clone(), e))?;
            let state = write_raw_with_state(&body, &dest)?;
            out.push(RenderedFile { path: dest, state });
        }
    }
    Ok(())
}

/// Read-only counterpart to [`regenerate_benches`]: classifies each
/// bundle's findings file(s) against on-disk content.
fn check_benches(
    project: &MockspaceProject,
    out_dir: &Path,
    report: &mut CheckReport,
) -> Result<(), RegenerateError> {
    let benches_root = project.root().join("mock").join(BENCHES_DIR);
    let bundles = match collect_bench_bundles(&benches_root)? {
        Some(b) => b,
        None => return Ok(()),
    };

    for bundle in bundles {
        for output in bundle.outputs {
            let dest = out_dir.join(BENCHES_DIR).join(&output.relative_dest);
            let body = fs::read_to_string(&output.source)
                .map_err(|e| RegenerateError::io(output.source.clone(), e))?;
            classify_against_disk(&dest, &body, report)?;
        }
    }
    Ok(())
}

/// One rendered output for a bench bundle: source path on disk
/// plus the destination path relative to `out_dir/benches/`.
struct BenchOutput {
    source: PathBuf,
    /// `<bundle>.md` for single-size, `<bundle>/<size>.md` for
    /// multi-size. Caller joins onto `out_dir/benches/`.
    relative_dest: PathBuf,
}

/// One bench bundle's render outputs.
struct BenchBundle {
    outputs: Vec<BenchOutput>,
}

/// Walk `benches_root` and classify each direct subdirectory as a
/// bench bundle. Returns `None` when `benches_root` itself does not
/// exist (project has no benches yet). Empty `Some(vec![])` means
/// the directory exists but contains no readable bundles.
fn collect_bench_bundles(
    benches_root: &Path,
) -> Result<Option<Vec<BenchBundle>>, RegenerateError> {
    let entries = match fs::read_dir(benches_root) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(RegenerateError::io(benches_root, e)),
    };

    // Sort bundle directories alphabetically so the rendered output
    // order is deterministic across runs.
    let mut bundle_paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    bundle_paths.sort();

    let mut bundles = Vec::with_capacity(bundle_paths.len());
    for bundle_dir in bundle_paths {
        let bundle_name = match bundle_dir.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let outputs = classify_bench_bundle(&bundle_dir, &bundle_name)?;
        if !outputs.is_empty() {
            bundles.push(BenchBundle { outputs });
        }
    }
    Ok(Some(bundles))
}

/// Inspect one bench bundle directory and return the render outputs.
///
/// Precedence rule (per the slice plan memo):
///
/// 1. If `<bundle>/findings.md` exists, return a single-size output
///    regardless of any sibling `<size>_findings.md` files. The
///    per-size files are dropped.
/// 2. Otherwise, gather every `<size>_findings.md` and return one
///    output per size. Sorted alphabetically by size token for
///    deterministic render order.
/// 3. If neither shape is present, return empty (the bundle is
///    silently skipped).
fn classify_bench_bundle(
    bundle_dir: &Path,
    bundle_name: &str,
) -> Result<Vec<BenchOutput>, RegenerateError> {
    let single = bundle_dir.join("findings.md");
    if single.is_file() {
        return Ok(vec![BenchOutput {
            source: single,
            relative_dest: PathBuf::from(format!("{bundle_name}.md")),
        }]);
    }

    let entries = match fs::read_dir(bundle_dir) {
        Ok(e) => e,
        Err(e) => return Err(RegenerateError::io(bundle_dir, e)),
    };

    let mut size_files: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(size) = file_name.strip_suffix(SIZE_FINDINGS_SUFFIX) {
            if !size.is_empty() {
                size_files.push((size.to_string(), path));
            }
        }
    }
    size_files.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(size_files
        .into_iter()
        .map(|(size, source)| BenchOutput {
            source,
            relative_dest: PathBuf::from(bundle_name).join(format!("{size}.md")),
        })
        .collect())
}

/// Atomic-write helper for raw (non-template-rendered) byte content.
/// Mirrors [`write_with_state`] but skips the template render step;
/// used for outputs like the dep-graph `.dot` and rendered SVG where
/// the source is already a fully formed string.
///
/// Parent-dir creation and byte-classification happen here; the
/// underlying atomic-rename is delegated to
/// [`mockspace_template::write_atomic`] for consistency with the
/// template-render path.
fn write_raw_with_state(
    content: &str,
    dest: &Path,
) -> Result<WriteState, RegenerateError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| RegenerateError::io(parent, e))?;
    }

    let state = match fs::read_to_string(dest) {
        Ok(existing) if existing == content => return Ok(WriteState::Unchanged),
        Ok(_) => WriteState::Updated,
        Err(e) if e.kind() == io::ErrorKind::NotFound => WriteState::Created,
        Err(e) => return Err(RegenerateError::io(dest, e)),
    };

    write_atomic(content, dest)?;
    Ok(state)
}

/// Render the templates in memory and compare against the existing
/// files in `out_dir`, covering both mock-root and per-crate output.
///
/// Read-only; never writes. Suitable for CI gating: pair with
/// [`CheckReport::needs_regen`] to fail builds when the rendered output
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

        classify_against_disk(&dest, &rendered, &mut report)?;
    }

    check_per_crate(project, out_dir, &mut report)?;
    check_dep_graph(&context.dep_graph, out_dir, &mut report)?;
    check_benches(project, out_dir, &mut report)?;

    Ok(report)
}

// ---------------------------------------------------------------------------
// Helpers (internal; not part of the public surface).

/// Context handed to mock-root templates.
///
/// `crates` carries per-workspace-member summaries from each crate's
/// `README.md.tmpl`; `dep_graph` is a placeholder string today and will
/// be populated by the dependency-graph rendering work.
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

/// Context handed to per-crate templates. One built per workspace
/// member; surfaces the crate's identity and dep-graph position so
/// templates can introspect their own relationship to the workspace.
#[derive(Serialize)]
struct PerCrateContext {
    name: String,
    deps: Vec<String>,
    is_proc_macro: bool,
}

impl PerCrateContext {
    fn from_info(info: &CrateInfo) -> Self {
        Self {
            name: info.name.clone(),
            deps: info.deps.clone(),
            is_proc_macro: info.is_proc_macro,
        }
    }
}

/// One per-crate render task. Captures the registered template-engine
/// key (so [`TemplateEnv::get_template`] can look it up), the source
/// path (for diagnostics), and the destination path under `out_dir`.
struct PerCrateTask {
    key: String,
    dest: PathBuf,
}

/// Walks each workspace-member crate's `mock/crates/<name>/` directory,
/// renders the leaf templates and any deepdives that exist, and
/// appends each outcome to `out`.
fn regenerate_per_crate(
    project: &MockspaceProject,
    out_dir: &Path,
    out: &mut Vec<RenderedFile>,
) -> Result<(), RegenerateError> {
    let mock_root = project.root().join("mock");
    for info in &project.crate_graph().crates {
        if !info.is_workspace_member {
            continue;
        }
        let crate_mock = mock_root.join("crates").join(&info.name);
        let crate_out = out_dir.join(&info.name);
        let context = PerCrateContext::from_info(info);

        let (env, tasks) = build_per_crate_env(&crate_mock, &crate_out)?;
        for task in tasks {
            let template = env.get_template(&task.key)?;
            let state = write_with_state(&template, &context, &task.dest)?;
            out.push(RenderedFile { path: task.dest, state });
        }
    }
    Ok(())
}

/// Read-only counterpart to [`regenerate_per_crate`]: walks the same
/// templates, renders to memory, and classifies each destination on
/// the supplied [`CheckReport`].
fn check_per_crate(
    project: &MockspaceProject,
    out_dir: &Path,
    report: &mut CheckReport,
) -> Result<(), RegenerateError> {
    let mock_root = project.root().join("mock");
    for info in &project.crate_graph().crates {
        if !info.is_workspace_member {
            continue;
        }
        let crate_mock = mock_root.join("crates").join(&info.name);
        let crate_out = out_dir.join(&info.name);
        let context = PerCrateContext::from_info(info);

        let (env, tasks) = build_per_crate_env(&crate_mock, &crate_out)?;
        for task in tasks {
            let template = env.get_template(&task.key)?;
            let rendered = template.render(&context)?;
            classify_against_disk(&task.dest, &rendered, report)?;
        }
    }
    Ok(())
}

/// Build a per-crate template env: registers each present leaf template
/// + every deepdive `*.md.tmpl`. Templates that don't exist on disk are
/// silently skipped (per-crate templates are optional). Returns the env
/// alongside the list of (registered-key, destination-path) pairs that
/// the caller iterates.
fn build_per_crate_env(
    crate_mock: &Path,
    crate_out: &Path,
) -> Result<(TemplateEnv, Vec<PerCrateTask>), RegenerateError> {
    let mut env = TemplateEnv::new();
    let mut tasks = Vec::new();

    for leaf in CRATE_LEAF_TEMPLATES {
        let src = crate_mock.join(leaf);
        let body = match fs::read_to_string(&src) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(RegenerateError::io(src, e)),
        };
        env.add_template(leaf, &body)?;
        tasks.push(PerCrateTask {
            key: (*leaf).to_string(),
            dest: crate_out.join(strip_tmpl_suffix(leaf)),
        });
    }

    let deepdives_dir = crate_mock.join("deepdives");
    match fs::read_dir(&deepdives_dir) {
        Ok(entries) => {
            // Collect into a Vec so we can sort for deterministic
            // rendering order (matters for the test suite and any
            // caller that walks the report sequentially).
            let mut sorted: Vec<PathBuf> = entries
                .filter_map(|res| res.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| n.ends_with(".md.tmpl"))
                })
                .collect();
            sorted.sort();

            for src in sorted {
                let file_name = src
                    .file_name()
                    .and_then(|s| s.to_str())
                    .expect("deepdive template path filtered to .md.tmpl above")
                    .to_string();
                let body = fs::read_to_string(&src)
                    .map_err(|e| RegenerateError::io(src.clone(), e))?;
                let key = format!("deepdives/{file_name}");
                env.add_template(&key, &body)?;
                tasks.push(PerCrateTask {
                    key,
                    dest: crate_out.join("deepdives").join(strip_tmpl_suffix(&file_name)),
                });
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(RegenerateError::io(deepdives_dir, e)),
    }

    Ok((env, tasks))
}

/// Compare a rendered string against `dest`'s on-disk content and
/// classify the result into the report. Shared by mock-root and
/// per-crate `check` paths.
fn classify_against_disk(
    dest: &Path,
    rendered: &str,
    report: &mut CheckReport,
) -> Result<(), RegenerateError> {
    match fs::read_to_string(dest) {
        Ok(existing) if existing == rendered => report.matched.push(dest.to_path_buf()),
        Ok(_) => report.drifted.push(dest.to_path_buf()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => report.missing.push(dest.to_path_buf()),
        Err(e) => return Err(RegenerateError::io(dest, e)),
    }
    Ok(())
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

    let dep_graph = dep_graph::render_dot(project.crate_graph());

    Ok(RenderContext { crates, dep_graph })
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

        // Mock-root templates plus dep-graph.dot (always emitted).
        // dep-graph.svg also lands when the test host has the
        // Graphviz `dot` binary on PATH; asserting on at-least
        // count keeps the test portable.
        assert!(report.files.len() >= 4, "expected >=4 files, got {}", report.files.len());
        assert!(out.join("DESIGN.md").is_file());
        assert!(out.join("PRINCIPLES.md").is_file());
        assert!(out.join("WORKFLOW.md").is_file());
        assert!(out.join("dep-graph.dot").is_file());
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

        // Mock-root templates plus dep-graph.dot. SVG is best-effort
        // and not classified by check (see check_dep_graph doc).
        assert_eq!(report.matched.len(), 4);
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
        // 4 minus the drifted PRINCIPLES.md = 3 matched (DESIGN.md, WORKFLOW.md, dep-graph.dot).
        assert_eq!(report.matched.len(), 3);
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
        // 3 mock-root + dep-graph.dot.
        assert_eq!(report.missing.len(), 4);
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
        assert!(report.files.len() >= 4, "expected >=4 files, got {}", report.files.len());
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

    // -----------------------------------------------------------------
    // Per-crate walk tests.

    fn write_crate_template(root: &Path, crate_name: &str, leaf: &str, body: &str) {
        let dir = root.join("mock").join("crates").join(crate_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(leaf), body).unwrap();
    }

    fn write_crate_deepdive(root: &Path, crate_name: &str, topic: &str, body: &str) {
        let dir = root.join("mock").join("crates").join(crate_name).join("deepdives");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{topic}.md.tmpl")), body).unwrap();
    }

    #[test]
    fn regenerate_renders_per_crate_design_and_backlog() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_crate_template(tmp.path(), "alpha", "DESIGN.md.tmpl", "alpha design {{ name }}");
        write_crate_template(tmp.path(), "alpha", "BACKLOG.md.tmpl", "alpha backlog");
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        let report = regenerate(&proj, &out).expect("regenerate");

        // 3 mock-root + 2 per-crate + dep-graph.dot (+ optional .svg
        // when Graphviz is on PATH) >= 6.
        assert!(report.files.len() >= 6, "expected >=6 files, got {}", report.files.len());
        let design = fs::read_to_string(out.join("alpha").join("DESIGN.md")).unwrap();
        assert_eq!(design, "alpha design alpha");
        let backlog = fs::read_to_string(out.join("alpha").join("BACKLOG.md")).unwrap();
        assert_eq!(backlog, "alpha backlog");
    }

    #[test]
    fn regenerate_renders_deepdives_under_subdir() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_crate_deepdive(tmp.path(), "alpha", "memory-model", "deep dive on memory");
        write_crate_deepdive(tmp.path(), "alpha", "thread-pool", "deep dive on threads");
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        let report = regenerate(&proj, &out).expect("regenerate");
        let deepdive_dir = out.join("alpha").join("deepdives");
        assert!(deepdive_dir.join("memory-model.md").is_file());
        assert!(deepdive_dir.join("thread-pool.md").is_file());

        // Order is alphabetical thanks to the deepdive sort.
        let per_crate: Vec<&PathBuf> = report
            .files
            .iter()
            .filter_map(|f| {
                if f.path.starts_with(&deepdive_dir) {
                    Some(&f.path)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(per_crate.len(), 2);
        assert!(per_crate[0].ends_with("memory-model.md"));
        assert!(per_crate[1].ends_with("thread-pool.md"));
    }

    #[test]
    fn regenerate_tolerates_crate_with_no_templates() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &["alpha"]);
        // Note: no per-crate templates written.
        let out = tmp.path().join("docs");

        let report = regenerate(&proj, &out).expect("regenerate");
        // Mock-root files plus dep-graph.dot; per-crate walk silently
        // produces zero (no per-crate templates on disk).
        assert!(report.files.len() >= 4, "expected >=4 files, got {}", report.files.len());
        assert!(!out.join("alpha").exists());
    }

    #[test]
    fn regenerate_skips_shame_template() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        // SHAME.md.tmpl present alongside DESIGN.md.tmpl.
        write_crate_template(tmp.path(), "alpha", "DESIGN.md.tmpl", "alpha design");
        write_crate_template(
            tmp.path(),
            "alpha",
            "SHAME.md.tmpl",
            "internal accounting, must not leak",
        );
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        assert!(out.join("alpha").join("DESIGN.md").is_file());
        assert!(
            !out.join("alpha").join("SHAME.md").exists(),
            "SHAME.md.tmpl must never reach the rendered docs tree"
        );
    }

    #[test]
    fn regenerate_per_crate_context_carries_deps() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_crate_template(
            tmp.path(),
            "beta",
            "DESIGN.md.tmpl",
            "beta deps: {% for d in deps %}{{ d }} {% endfor %}",
        );
        // Hand-build a project with a dep edge alpha → beta.
        let alpha = CrateInfo {
            name: "alpha".to_string(),
            root_path: tmp.path().join("alpha"),
            is_proc_macro: false,
            is_workspace_member: true,
            deps: Vec::new(),
        };
        let beta = CrateInfo {
            name: "beta".to_string(),
            root_path: tmp.path().join("beta"),
            is_proc_macro: false,
            is_workspace_member: true,
            deps: vec!["alpha".to_string()],
        };
        let mut by_name = std::collections::HashMap::new();
        by_name.insert("alpha".to_string(), 0);
        by_name.insert("beta".to_string(), 1);
        let graph = CrateGraph {
            crates: vec![alpha, beta],
            by_name,
        };
        let proj = ProjectBuilder::new(tmp.path().to_path_buf(), RunSurface::Ci, Gate::Commit)
            .with_crate_graph(graph)
            .build();
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let body = fs::read_to_string(out.join("beta").join("DESIGN.md")).unwrap();
        assert_eq!(body, "beta deps: alpha ");
    }

    #[test]
    fn regenerate_per_crate_classifies_states() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_crate_template(tmp.path(), "alpha", "DESIGN.md.tmpl", "v1");
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        let first = regenerate(&proj, &out).expect("first");
        // Find the per-crate file in the first report.
        let alpha_first = first
            .files
            .iter()
            .find(|f| f.path.ends_with("alpha/DESIGN.md"))
            .unwrap();
        assert_eq!(alpha_first.state, WriteState::Created);

        // Re-render same body: Unchanged.
        let second = regenerate(&proj, &out).expect("second");
        let alpha_second = second
            .files
            .iter()
            .find(|f| f.path.ends_with("alpha/DESIGN.md"))
            .unwrap();
        assert_eq!(alpha_second.state, WriteState::Unchanged);

        // Change body: Updated.
        write_crate_template(tmp.path(), "alpha", "DESIGN.md.tmpl", "v2");
        let third = regenerate(&proj, &out).expect("third");
        let alpha_third = third
            .files
            .iter()
            .find(|f| f.path.ends_with("alpha/DESIGN.md"))
            .unwrap();
        assert_eq!(alpha_third.state, WriteState::Updated);
    }

    #[test]
    fn regenerate_writes_dep_graph_dot() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        let proj = project(tmp.path(), &["alpha", "beta"]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let dot = fs::read_to_string(out.join("dep-graph.dot")).expect("dep-graph.dot");
        assert!(
            dot.starts_with("digraph workspace {\n"),
            "expected dot header, got: {dot}"
        );
        assert!(dot.contains("\"alpha\""));
        assert!(dot.contains("\"beta\""));
    }

    // -----------------------------------------------------------------
    // Bench-findings render tests.

    fn write_bench_findings(root: &Path, bundle: &str, file_name: &str, body: &str) {
        let dir = root.join("mock").join("benches").join(bundle);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(file_name), body).unwrap();
    }

    #[test]
    fn regenerate_renders_single_size_bench() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_bench_findings(tmp.path(), "alpha", "findings.md", "alpha findings");
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let body = fs::read_to_string(out.join("benches").join("alpha.md")).unwrap();
        assert_eq!(body, "alpha findings");
        // No nested subdir for single-size.
        assert!(!out.join("benches").join("alpha").exists());
    }

    #[test]
    fn regenerate_renders_multi_size_bench() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_bench_findings(tmp.path(), "alpha", "64_findings.md", "n=64");
        write_bench_findings(tmp.path(), "alpha", "256_findings.md", "n=256");
        write_bench_findings(tmp.path(), "alpha", "1024_findings.md", "n=1024");
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let dir = out.join("benches").join("alpha");
        assert_eq!(fs::read_to_string(dir.join("64.md")).unwrap(), "n=64");
        assert_eq!(fs::read_to_string(dir.join("256.md")).unwrap(), "n=256");
        assert_eq!(fs::read_to_string(dir.join("1024.md")).unwrap(), "n=1024");
        // No flat single-size file for multi-size.
        assert!(!out.join("benches").join("alpha.md").exists());
    }

    #[test]
    fn regenerate_prefers_single_size_when_both_shapes_present() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_bench_findings(tmp.path(), "alpha", "findings.md", "aggregate");
        write_bench_findings(tmp.path(), "alpha", "64_findings.md", "n=64");
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        assert_eq!(
            fs::read_to_string(out.join("benches").join("alpha.md")).unwrap(),
            "aggregate"
        );
        assert!(
            !out.join("benches").join("alpha").exists(),
            "multi-size files must be dropped when single-size wins"
        );
    }

    #[test]
    fn regenerate_skips_bundle_with_no_findings() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        // Create the bundle directory but no findings files at all.
        fs::create_dir_all(tmp.path().join("mock").join("benches").join("alpha")).unwrap();
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        assert!(
            !out.join("benches").exists(),
            "no findings means no docs/benches/ subdir at all"
        );
    }

    #[test]
    fn regenerate_tolerates_missing_benches_dir() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        // No mock/benches/ created at all.
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        assert!(!out.join("benches").exists());
    }

    #[test]
    fn regenerate_renders_multiple_bundles_alphabetically() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_bench_findings(tmp.path(), "zeta", "findings.md", "z");
        write_bench_findings(tmp.path(), "alpha", "findings.md", "a");
        write_bench_findings(tmp.path(), "mango", "findings.md", "m");
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        let report = regenerate(&proj, &out).expect("regenerate");
        let bench_paths: Vec<&PathBuf> = report
            .files
            .iter()
            .filter(|f| f.path.starts_with(out.join("benches")))
            .map(|f| &f.path)
            .collect();
        assert_eq!(bench_paths.len(), 3);
        assert!(bench_paths[0].ends_with("alpha.md"));
        assert!(bench_paths[1].ends_with("mango.md"));
        assert!(bench_paths[2].ends_with("zeta.md"));
    }

    #[test]
    fn check_classifies_bench_findings_drift() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_bench_findings(tmp.path(), "alpha", "findings.md", "original");
        let proj = project(tmp.path(), &[]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("seed");
        let clean = check(&proj, &out).expect("check matches");
        assert!(!clean.needs_regen());
        assert!(clean
            .matched
            .iter()
            .any(|p| p.ends_with("benches/alpha.md")));

        // Hand-edit to simulate stale committed copy.
        fs::write(out.join("benches").join("alpha.md"), "stale").unwrap();
        let drifted = check(&proj, &out).expect("check drift");
        assert!(drifted.needs_regen());
        assert!(drifted
            .drifted
            .iter()
            .any(|p| p.ends_with("benches/alpha.md")));
    }

    #[test]
    fn dep_graph_context_is_populated_for_templates() {
        // A mock-root template that references {{ dep_graph }} must
        // see the rendered .dot source, not the slice 2 empty
        // placeholder.
        let tmp = TempDir::new().unwrap();
        let mock = tmp.path().join("mock");
        fs::create_dir_all(&mock).unwrap();
        fs::write(
            mock.join("DESIGN.md.tmpl"),
            "graph:\n{{ dep_graph }}",
        )
        .unwrap();
        fs::write(mock.join("PRINCIPLES.md.tmpl"), "principles").unwrap();
        fs::write(mock.join("WORKFLOW.md.tmpl"), "workflow").unwrap();
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        regenerate(&proj, &out).expect("regenerate");
        let design = fs::read_to_string(out.join("DESIGN.md")).unwrap();
        assert!(
            design.contains("digraph workspace"),
            "DESIGN.md should embed the dep-graph source, got: {design}"
        );
        assert!(design.contains("\"alpha\""));
    }

    #[test]
    fn check_classifies_per_crate_output() {
        let tmp = TempDir::new().unwrap();
        write_mock_root_templates(tmp.path());
        write_crate_template(tmp.path(), "alpha", "DESIGN.md.tmpl", "alpha");
        let proj = project(tmp.path(), &["alpha"]);
        let out = tmp.path().join("docs");

        // No regenerate first: per-crate file is missing on disk.
        let report = check(&proj, &out).expect("check");
        assert!(report.needs_regen());
        assert!(report
            .missing
            .iter()
            .any(|p| p.ends_with("alpha/DESIGN.md")));

        // Seed, then check again: matched.
        regenerate(&proj, &out).expect("seed");
        let report = check(&proj, &out).expect("check after seed");
        assert!(!report.needs_regen());
        assert!(report.matched.iter().any(|p| p.ends_with("alpha/DESIGN.md")));
    }
}
