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

use crate::crate_graph::build_crate_graph;
use crate::design_rounds::discover_design_rounds;
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
        .filter_entry(|e| !is_skipped(e, root))
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
        // Read bytes; decode utf8 strictly for Rust (the AST primitives
        // need it to be valid utf8); decode utf8-lossy for Markdown / Toml
        // where a stray non-utf8 byte should not abort the whole project
        // scan. The lossy path replaces invalid sequences with U+FFFD, which
        // is fine for content-regex / token-scan / strip-based primitives.
        let bytes = std::fs::read(path).map_err(|e| ParseError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let source = match language {
            Language::Rust => String::from_utf8(bytes).map_err(|e| ParseError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })?,
            _ => String::from_utf8_lossy(&bytes).into_owned(),
        };
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

    // Crate graph: walk Cargo.toml files independently. Populating the
    // proc_macro_crates set on WorkspaceMetadata so stack-lints can skip
    // proc-macro source as required.
    let crate_graph = build_crate_graph(root);
    let proc_macro_crates: std::collections::HashSet<String> = crate_graph
        .crates
        .iter()
        .filter(|c| c.is_proc_macro)
        .map(|c| c.name.clone())
        .collect();
    let design_rounds = discover_design_rounds(root);

    // Mutate only the focused fields rather than replacing the whole
    // WorkspaceMetadata so any state the builder accumulates between
    // calls (`task_state` etc.) survives.
    let builder = builder
        .with_crate_graph(crate_graph)
        .with_proc_macro_crates(proc_macro_crates)
        .with_design_rounds(design_rounds);

    Ok(builder.build())
}

/// Filename / directory filter for the walker's `filter_entry`. Takes a
/// `&walkdir::DirEntry` so the cached `file_type()` from the readdir
/// result is used in preference to a fresh `Path::is_dir()` syscall.
fn is_skipped(entry: &walkdir::DirEntry, root: &Path) -> bool {
    let path = entry.path();
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    let file_type = entry.file_type();
    if file_type.is_dir() {
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
    // Fall back to the first path component when it's a directory
    // (e.g. `crates/foo/...` workspaces without an enclosing `crates`
    // segment). When the relative path has only a single component
    // (the file itself, e.g. a top-level Cargo.toml or README.md),
    // return the documented sentinel "workspace" rather than the
    // file basename.
    let mut comps = relative.components();
    let Some(first) = comps.next() else {
        return "workspace".to_string();
    };
    if comps.next().is_some() {
        first.as_os_str().to_string_lossy().to_string()
    } else {
        "workspace".to_string()
    }
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
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            "[package]\nname=\"foo\"",
        );
        write(&tmp.path().join("target/debug/something.rs"), "skipped");

        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let docs: Vec<_> = project.documents().collect();
        // Three files counted: lib.rs, README.md, Cargo.toml. target/ skipped.
        assert_eq!(
            docs.len(),
            3,
            "got: {:?}",
            docs.iter().map(|d| d.path()).collect::<Vec<_>>()
        );
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
    fn crate_name_for_top_level_file_is_workspace_sentinel() {
        // Single-component relative path (file directly at workspace root)
        // returns "workspace" rather than the file basename. Regression
        // guard against the prior behaviour where top-level Cargo.toml
        // was attributed to crate name "Cargo.toml".
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("Cargo.toml"), "[workspace]");
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let doc = project.documents().next().unwrap();
        assert_eq!(doc.crate_name(), "workspace");
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

    #[test]
    fn populates_crate_graph_and_proc_macro_set() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"",
        );
        write(
            &tmp.path().join("crates/mac/Cargo.toml"),
            "[package]\nname = \"mac\"\nversion = \"0.1.0\"\n\n[lib]\nproc-macro = true",
        );
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        assert!(project.crate_graph().get("foo").is_some());
        assert!(project.crate_graph().is_proc_macro("mac"));
        assert!(project.workspace().proc_macro_crates.contains("mac"));
        assert!(!project.workspace().proc_macro_crates.contains("foo"));
    }

    #[test]
    fn populates_design_rounds_view() {
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/202605211200");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("topic.md"), "# topic");
        let project = scope_walk(tmp.path(), RunSurface::Local).unwrap();
        let rounds = project.design_rounds();
        assert_eq!(rounds.rounds.len(), 1);
        assert_eq!(rounds.rounds[0].timestamp, "202605211200");
    }
}
