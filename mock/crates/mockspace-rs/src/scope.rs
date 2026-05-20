//! Project filesystem walker.
//!
//! Walks a workspace root, classifies files by extension into [`Language`],
//! reads source bytes, computes BLAKE3 content hashes, and assembles a
//! [`MockspaceProject`] ready for engine dispatch.
//!
//! The walker is intentionally narrow: it skips `target/`, `.git/`, and
//! `node_modules/` by default; it does not parse `cargo metadata` to build
//! the crate graph (that's a follow-up); it does not parse `mock/design_rounds/`
//! into a typed view (also a follow-up). The shipped surface is enough to
//! let `cargo mock lint` walk the source tree and run the catalog against
//! every Rust file, which is the load-bearing 90% of the value.

use std::path::Path;

use mockspace_core::lint::{ContentHash, Gate, Language, RunSurface};
use walkdir::WalkDir;

use crate::document::MockspaceDocument;
use crate::errors::ParseError;
use crate::project::{MockspaceProject, ProjectBuilder};

/// Walk the workspace root and build a project. Path classification:
///
/// - `.rs` → `Language::Rust`.
/// - `.md` / `.md.tmpl` → `Language::Markdown`.
/// - `.ts` / `.tsx` → `Language::TypeScript`.
/// - `.toml` → `Language::Toml`.
/// - Other extensions: skipped silently.
///
/// Skipped directories: `target`, `.git`, `node_modules`, `.cargo`, `dist`,
/// `build`. Hidden files (`.*`) are skipped except the .md.tmpl convention
/// is honoured via the explicit extension rule.
pub fn scope_walk(root: &Path, surface: RunSurface) -> Result<MockspaceProject, ParseError> {
    let mut builder = ProjectBuilder::new(root.to_path_buf(), surface, Gate::Commit);
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_skipped(e.path(), root))
    {
        let entry = entry.map_err(|e| ParseError::Io {
            path: e.path().map(|p| p.to_path_buf()).unwrap_or_default(),
            source: io_from_walkdir(e),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(language) = classify(path) else {
            continue;
        };
        let source = std::fs::read_to_string(path).map_err(|e| ParseError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let content_hash = blake3_hash(&source);
        let crate_name = derive_crate_name(path, root);
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        builder.push_document(MockspaceDocument::with_hash(
            relative,
            crate_name,
            language,
            source,
            content_hash,
        ));
    }
    if surface == RunSurface::Editor {
        builder.mark_all_staged();
    }
    Ok(builder.build())
}

/// Filename / directory filter for the walker's `filter_entry`.
fn is_skipped(path: &Path, root: &Path) -> bool {
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    // Skip well-known build / vendor / vcs directories.
    if path.is_dir() {
        matches!(
            name,
            "target"
                | ".git"
                | "node_modules"
                | ".cargo"
                | "dist"
                | "build"
                | ".idea"
                | ".vscode"
                | ".direnv"
        )
    } else {
        // Hidden files at file level: skip unless ext-based classification
        // would have caught them (which it does for `.md.tmpl`).
        name.starts_with('.') && classify(path).is_none()
    }
}

fn classify(path: &Path) -> Option<Language> {
    let name = path.file_name()?.to_str()?;
    if let Some(stem) = name.strip_suffix(".md.tmpl") {
        let _ = stem;
        return Some(Language::Markdown);
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some(Language::Rust),
        "md" | "markdown" => Some(Language::Markdown),
        "ts" | "tsx" => Some(Language::TypeScript),
        "toml" => Some(Language::Toml),
        _ => None,
    }
}

fn blake3_hash(source: &str) -> ContentHash {
    let mut h = blake3::Hasher::new();
    h.update(source.as_bytes());
    let digest = h.finalize();
    ContentHash(*digest.as_bytes())
}

/// Best-effort: derive the crate name from path components. Looks for
/// `mock/crates/<name>/` or `crates/<name>/` patterns in the path; falls
/// back to the top-level directory name. Production engines that parse
/// `cargo metadata` provide a precise mapping; this is a stopgap until
/// the crate graph is wired (Phase 2C).
fn derive_crate_name(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut after_crates = false;
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy();
        if after_crates {
            return name.to_string();
        }
        if name == "crates" {
            after_crates = true;
        }
    }
    // Fall back to the first path component, or "workspace" if none.
    relative
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string())
}

fn io_from_walkdir(e: walkdir::Error) -> std::io::Error {
    // Preserve the original io::Error chain when walkdir wraps one;
    // otherwise wrap the walkdir message in a fresh io::Error.
    e.into_io_error()
        .unwrap_or_else(|| std::io::Error::other("walkdir error without io::Error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write(p: &Path, contents: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn walks_rust_and_markdown_files() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("crates/foo/src/lib.rs"), "fn x() {}");
        write(&tmp.path().join("README.md"), "# hi");
        write(&tmp.path().join("crates/foo/Cargo.toml"), "[package]\nname=\"foo\"");
        write(&tmp.path().join("target/debug/something.rs"), "skipped");

        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let docs: Vec<_> = project.documents().collect();
        // Three files counted: lib.rs, README.md, Cargo.toml. target/ skipped.
        assert_eq!(docs.len(), 3, "got: {:?}", docs.iter().map(|d| d.path()).collect::<Vec<_>>());
        assert!(docs.iter().any(|d| d.language() == Language::Rust));
        assert!(docs.iter().any(|d| d.language() == Language::Markdown));
        assert!(docs.iter().any(|d| d.language() == Language::Toml));
    }

    #[test]
    fn content_hash_is_blake3() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("a.rs"), "hello");
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let doc = project.documents().next().unwrap();
        let expected = {
            let mut h = blake3::Hasher::new();
            h.update(b"hello");
            *h.finalize().as_bytes()
        };
        assert_eq!(doc.content_hash().0, expected);
    }

    #[test]
    fn crate_name_derived_from_mock_crates_layout() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("mock/crates/arvo/src/lib.rs"), "");
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let doc = project.documents().next().unwrap();
        assert_eq!(doc.crate_name(), "arvo");
    }

    #[test]
    fn editor_surface_marks_all_staged() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("a.rs"), "");
        write(&tmp.path().join("b.rs"), "");
        let project = scope_walk(tmp.path(), RunSurface::Editor).unwrap();
        assert_eq!(project.staged_documents().count(), 2);
    }

    #[test]
    fn md_tmpl_recognised_as_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("DESIGN.md.tmpl"), "# design");
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let doc = project.documents().next().unwrap();
        assert_eq!(doc.language(), Language::Markdown);
    }
}
