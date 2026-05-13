//! Directory-walking renderers: `AgentRenderer` and `walk_template_tree`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::RenderError;
use crate::platform::Platform;
use crate::template::TemplateEnv;

#[derive(Debug, Default)]
pub struct RenderReport {
    pub entries: Vec<RenderedFile>,
    pub total_bytes: u64,
    pub total_duration: Duration,
}

#[derive(Debug)]
pub struct RenderedFile {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub bytes_written: u64,
    pub duration: Duration,
}

/// Walks a source directory of `.md.tmpl` (or `.tmpl`) files and emits
/// the rendered output to platform-derived paths under `dst_root`.
pub struct AgentRenderer<'env, P: Platform> {
    env: &'env mut TemplateEnv,
    platform: P,
    src_root: PathBuf,
    dst_root: PathBuf,
}

impl<'env, P: Platform> AgentRenderer<'env, P> {
    pub fn new(
        env: &'env mut TemplateEnv,
        platform: P,
        src_root: PathBuf,
        dst_root: PathBuf,
    ) -> Self {
        Self {
            env,
            platform,
            src_root,
            dst_root,
        }
    }

    pub fn render_all<C: Serialize>(
        &mut self,
        ctx: &C,
    ) -> Result<RenderReport, RenderError> {
        let started = Instant::now();
        let mut report = RenderReport::default();
        let mut templates = Vec::new();
        walk_inner(&self.src_root, &mut templates)?;
        for src in &templates {
            let logical = logical_name(&self.src_root, src)?;
            let dst = self.platform.output_path(&self.dst_root, &logical);
            let entry = render_one(self.env, src, &dst, &logical, ctx)?;
            report.total_bytes = report.total_bytes.saturating_add(entry.bytes_written);
            report.entries.push(entry);
        }
        report.total_duration = started.elapsed();
        Ok(report)
    }
}

/// Renders an entire source tree to `dst_root` preserving relative structure,
/// with `.tmpl` stripped from filenames. No platform-specific path rewriting.
pub fn walk_template_tree<C: Serialize>(
    env: &mut TemplateEnv,
    src_root: &Path,
    dst_root: &Path,
    ctx: &C,
) -> Result<RenderReport, RenderError> {
    let started = Instant::now();
    let mut report = RenderReport::default();
    let mut templates = Vec::new();
    walk_inner(src_root, &mut templates)?;
    for src in &templates {
        let rel = src.strip_prefix(src_root).unwrap_or(src);
        let dst_rel = strip_tmpl_extension(rel);
        let dst = dst_root.join(dst_rel);
        let logical = rel.to_string_lossy().into_owned();
        let entry = render_one(env, src, &dst, &logical, ctx)?;
        report.total_bytes = report.total_bytes.saturating_add(entry.bytes_written);
        report.entries.push(entry);
    }
    report.total_duration = started.elapsed();
    Ok(report)
}

fn render_one<C: Serialize>(
    env: &mut TemplateEnv,
    src: &Path,
    dst: &Path,
    logical: &str,
    ctx: &C,
) -> Result<RenderedFile, RenderError> {
    let started = Instant::now();
    let source = fs::read_to_string(src)?;
    // Register under logical name; render_str avoids needing a name at all,
    // which is friendlier for callers that don't care about template caching.
    let rendered = env.render_str(&source, ctx)?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = rendered.len() as u64;
    fs::write(dst, rendered)?;
    let _ = logical; // logical retained for future use (caching, diagnostics)
    Ok(RenderedFile {
        source: src.to_path_buf(),
        destination: dst.to_path_buf(),
        bytes_written: bytes,
        duration: started.elapsed(),
    })
}

fn walk_inner(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), RenderError> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_inner(&path, out)?;
        } else if has_tmpl_extension(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn has_tmpl_extension(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("tmpl")
}

fn logical_name(src_root: &Path, src: &Path) -> Result<String, RenderError> {
    let rel = src.strip_prefix(src_root).map_err(|_| {
        RenderError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "source path not under src_root",
        ))
    })?;
    let stripped = strip_tmpl_extension(rel);
    // Strip a trailing ".md" if present (so "rules/foo.md.tmpl" -> "rules/foo").
    let s = stripped.to_string_lossy();
    let s = s.strip_suffix(".md").unwrap_or(&s);
    Ok(s.replace('\\', "/"))
}

fn strip_tmpl_extension(path: &Path) -> PathBuf {
    if let Some(stem) = path.file_stem() {
        if has_tmpl_extension(path) {
            if let Some(parent) = path.parent() {
                return parent.join(stem);
            }
            return PathBuf::from(stem);
        }
    }
    path.to_path_buf()
}
