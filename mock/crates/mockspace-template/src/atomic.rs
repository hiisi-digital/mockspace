//! Atomic render-to-disk helper.
//!
//! Renders a registered [`Template`] against a context and writes
//! the output to a destination path via the standard "write to
//! sibling temp, fsync, rename" sequence. Idempotent: a render
//! whose bytes match the existing destination skips the write
//! entirely, preserving the destination's mtime.
//!
//! The temp file lives in the destination's parent directory so
//! the final rename is an atomic intra-filesystem operation. On
//! Unix this is a single `rename(2)`; on Windows the `tempfile`
//! crate's `persist` handles the equivalent move-with-replace.
//!
//! Slice 1 of the Phase 3 render pipeline orchestration per
//! `mock/research/202605222000_phase-3-render-pipeline-orchestration.md`.
//! The render-pipeline orchestrator (`mockspace_rs::render`,
//! slices 2-5) calls this helper once per output file.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use tempfile::NamedTempFile;

use crate::error::RenderError;
use crate::template::Template;

/// Render `template` with `context` and write the result to `dest`
/// atomically. Idempotent: if `dest` already exists and its
/// content byte-matches the rendered output, the write is skipped
/// and the destination's mtime is preserved.
///
/// Atomicity: the rendered output is written to a sibling temp
/// file in `dest.parent()`, fsync'd, and renamed over `dest`.
/// Same-filesystem rename guarantees a partial state never
/// becomes visible (a reader observing `dest` either sees the
/// old content or the new, never a partial write).
///
/// Errors:
///
/// - [`RenderError::Minijinja`]: the template body or context
///   triggered a template-engine error.
/// - [`RenderError::Io`]: filesystem operation failed. Common
///   causes: `dest.parent()` does not exist, parent is not
///   writable, dest is a directory rather than a file, the temp
///   file could not be created or renamed.
///
/// The caller is responsible for ensuring `dest.parent()` exists.
/// This helper does not auto-create directories: a missing parent
/// surfaces as an `Io` error rather than silently creating a
/// directory tree the caller may not have intended.
pub fn render_atomic<C: Serialize>(
    template: &Template<'_>,
    context: &C,
    dest: &Path,
) -> Result<(), RenderError> {
    let rendered = template.render(context)?;

    // Idempotency: if dest already holds these exact bytes, no
    // write happens. This preserves mtime (downstream tools that
    // gate on mtime, like cargo, skip work) and avoids needless
    // disk churn on identity-renders.
    //
    // `read_to_string` covers the UTF-8 case (which is every
    // template-rendered output today; minijinja produces UTF-8).
    // A non-UTF8 destination silently falls through to rewrite,
    // which is the right behaviour: if a file at this path is
    // non-text, it is not the previous render's output, so
    // overwriting is correct. Future callers that render binary
    // output through this helper need a byte-comparing variant.
    if let Ok(existing) = fs::read_to_string(dest) {
        if existing == rendered {
            return Ok(());
        }
    }

    let parent = dest.parent().ok_or_else(|| {
        RenderError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("dest has no parent directory: {}", dest.display()),
        ))
    })?;

    // The temp file is created in `parent` so the rename is
    // atomic. `NamedTempFile` schedules cleanup on drop if
    // `persist` does not succeed (i.e. on the error path).
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(rendered.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(dest).map_err(|e| RenderError::Io(e.error))?;
    Ok(())
}

/// Atomic-write a pre-rendered string to `dest`. Peer of
/// [`render_atomic`] for callers that already hold the rendered
/// bytes (e.g. dep-graph `.dot` source emitted directly rather than
/// via a template). Same atomicity and idempotency contract:
/// write-to-sibling-temp, fsync, rename; identical bytes skip the
/// write and preserve mtime.
pub fn write_atomic(content: &str, dest: &Path) -> Result<(), RenderError> {
    if let Ok(existing) = fs::read_to_string(dest) {
        if existing == content {
            return Ok(());
        }
    }

    let parent = dest.parent().ok_or_else(|| {
        RenderError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("dest has no parent directory: {}", dest.display()),
        ))
    })?;

    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(dest).map_err(|e| RenderError::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::TemplateEnv;
    use std::collections::HashMap;
    use std::time::Duration;
    use tempfile::TempDir;

    fn env_with(name: &str, source: &str) -> TemplateEnv {
        let mut env = TemplateEnv::new();
        env.add_template(name, source).expect("add template");
        env
    }

    fn ctx(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn render_atomic_writes_new_file() {
        let env = env_with("greet", "hello {{ name }}");
        let template = env.get_template("greet").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.txt");

        render_atomic(&template, &ctx(&[("name", "world")]), &dest).expect("render");
        let on_disk = fs::read_to_string(&dest).unwrap();
        assert_eq!(on_disk, "hello world");
    }

    #[test]
    fn render_atomic_is_idempotent_on_matching_content() {
        let env = env_with("greet", "hello {{ name }}");
        let template = env.get_template("greet").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.txt");

        render_atomic(&template, &ctx(&[("name", "world")]), &dest).expect("first render");
        let mtime_before = fs::metadata(&dest).unwrap().modified().unwrap();

        // Sleep past the filesystem's mtime granularity (1 second on
        // many filesystems is enough; this test occasionally needs
        // more on coarse filesystems).
        std::thread::sleep(Duration::from_millis(1100));

        render_atomic(&template, &ctx(&[("name", "world")]), &dest).expect("second render");
        let mtime_after = fs::metadata(&dest).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "mtime should be preserved when content is unchanged"
        );
    }

    #[test]
    fn render_atomic_rewrites_when_content_changes() {
        let env = env_with("greet", "hello {{ name }}");
        let template = env.get_template("greet").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.txt");

        render_atomic(&template, &ctx(&[("name", "world")]), &dest).expect("first");
        render_atomic(&template, &ctx(&[("name", "mockspace")]), &dest).expect("second");
        let on_disk = fs::read_to_string(&dest).unwrap();
        assert_eq!(on_disk, "hello mockspace");
    }

    #[test]
    fn render_atomic_errors_when_parent_missing() {
        let env = env_with("noop", "noop");
        let template = env.get_template("noop").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("nope").join("out.txt");

        let err = render_atomic(&template, &ctx(&[]), &dest).expect_err("should fail");
        assert!(
            matches!(err, RenderError::Io(_)),
            "expected io error, got {err:?}"
        );
    }

    #[test]
    fn render_atomic_errors_when_dest_is_directory() {
        let env = env_with("noop", "noop");
        let template = env.get_template("noop").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("subdir");
        fs::create_dir(&dest).unwrap();

        let err = render_atomic(&template, &ctx(&[]), &dest).expect_err("should fail");
        assert!(
            matches!(err, RenderError::Io(_)),
            "expected io error, got {err:?}"
        );
    }

    #[test]
    fn render_atomic_surfaces_minijinja_errors() {
        // Reference an undefined variable under Strict undefined
        // behaviour (set by `TemplateEnv::new`).
        let env = env_with("strict", "{{ missing }}");
        let template = env.get_template("strict").unwrap();
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("out.txt");

        let err = render_atomic(&template, &ctx(&[]), &dest).expect_err("should fail");
        assert!(
            matches!(err, RenderError::Minijinja(_)),
            "expected minijinja error, got {err:?}"
        );
        // Make sure no temp file leaked into the parent dir.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            leftovers.is_empty(),
            "expected no leftover temp files, got {:?}",
            leftovers.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }
}
