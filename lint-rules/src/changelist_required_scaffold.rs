//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `changelist-required` passes outside IMPL: scaffolding, and a nuke.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::staged_or_worktree;
use crate::src_layout::SrcLayout;

/// The marker a nuked crate's `lib.rs` stub carries.
const NUKE_MARKER: &str = "Nuked by";

/// Whether a source file the gate would otherwise refuse passes in a phase
/// other than IMPL, given the `content` it carries, `None` for a removal.
///
/// Three things pass. A removal from a crate being nuked, and the nuke stub
/// written as that crate's `lib.rs`, both judged by the whole shape a nuke
/// leaves. And scaffolding: a file declaring nothing, where `HEAD` does not
/// hold it or holds it declaring nothing too. Scaffolding is not the source
/// this gate protects: it wires a crate into the workspace so the dependency
/// graph, the layer numbering and the structure documents have real edges to
/// read, and it is how a design round adds a crate, its doc phase committing
/// the manifest and a `lib.rs` of the crate's documentation. A file that held
/// items and is cut down to a comment is a source change like any other, so
/// the judgement is of both sides. Any other text in a nuked crate, a build
/// script or a test beside the stub included, is judged as scaffolding.
pub(super) fn passes_outside_impl(
    workspace_root: &Path,
    layout: &SrcLayout,
    file: &str,
    content: Option<&str>,
) -> bool {
    match content {
        None => crate_is_nuked(workspace_root, layout, file),
        Some(text) => {
            let stub_at_lib = is_stub(text)
                && lib_of(layout, file).is_some_and(|(src, lib)| {
                    lib == file && nothing_else_left(workspace_root, &src, &lib)
                });
            stub_at_lib || (declares_nothing(text) && nothing_at_head(workspace_root, file))
        }
    }
}

/// Whether `text` is the stub a nuke writes over `lib.rs`.
fn is_stub(text: &str) -> bool {
    text.contains(NUKE_MARKER) && declares_nothing(text)
}

/// The `src` directory and `lib.rs` path of the package holding `file`.
fn lib_of(layout: &SrcLayout, file: &str) -> Option<(PathBuf, String)> {
    let src = layout.package_dir(Path::new(""), file)?.join("src");
    let lib = src.join("lib.rs").to_string_lossy().replace('\\', "/");
    Some((src, lib))
}

/// Whether `HEAD` holds nothing that declares anything at `file`: the path is
/// not in `HEAD`, or the blob there declares nothing.
fn nothing_at_head(workspace_root: &Path, file: &str) -> bool {
    let Ok(out) = Command::new("git")
        .args(["show", &format!("HEAD:./{file}")])
        .current_dir(workspace_root)
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        // Not in HEAD, or no HEAD yet: a file this change creates.
        return true;
    }
    String::from_utf8(out.stdout).is_ok_and(|s| declares_nothing(&s))
}

/// Whether the crate holding `file` is being nuked: what the commit carries as
/// its `lib.rs` is the nuke stub, and no other Rust file of its `src` is left.
///
/// The marker alone proves nothing, since writing it is itself a change the
/// scaffolding arm would judge. Only the whole shape is a nuke. The stub is
/// read from the index; a `lib.rs` changed past it in the working tree is a
/// changed file of its own and is judged on its own text.
fn crate_is_nuked(workspace_root: &Path, layout: &SrcLayout, file: &str) -> bool {
    lib_of(layout, file).is_some_and(|(src, lib)| {
        staged_or_worktree(workspace_root, &lib, "staged").is_some_and(|t| is_stub(&t))
            && nothing_else_left(workspace_root, &src, &lib)
    })
}

/// Whether `src` holds no Rust file other than `lib`, in the index or on disk.
fn nothing_else_left(workspace_root: &Path, src: &Path, lib: &str) -> bool {
    !other_source_in_index(workspace_root, src, lib) && !other_source_on_disk(&workspace_root.join(src))
}

/// Whether the index holds a Rust file under `src` other than `lib`, both
/// relative to `workspace_root`. A failing query proves nothing, so it answers yes.
fn other_source_in_index(workspace_root: &Path, src: &Path, lib: &str) -> bool {
    let Ok(out) = Command::new("git")
        .args(["ls-files", "-z", "--"])
        .arg(src)
        .current_dir(workspace_root)
        .output()
    else {
        return true;
    };
    if !out.status.success() {
        return true;
    }
    out.stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(String::from_utf8_lossy)
        .any(|p| p.ends_with(".rs") && p != lib)
}

/// Whether the directory `src` holds, at any depth, a Rust file other than its `lib.rs`.
fn other_source_on_disk(src: &Path) -> bool {
    let Ok(read) = std::fs::read_dir(src) else {
        return false;
    };
    read.flatten().any(|e| {
        let p = e.path();
        if p.is_dir() {
            walk_has_rust(&p)
        } else {
            p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "lib.rs")
        }
    })
}

/// Whether `dir` holds a Rust file at any depth.
fn walk_has_rust(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|read| {
        read.flatten().any(|e| {
            let p = e.path();
            if p.is_dir() { walk_has_rust(&p) } else { p.extension().is_some_and(|x| x == "rs") }
        })
    })
}

/// Whether a Rust file declares nothing at all.
///
/// Read from the parse tree: every top-level node is a comment or an inner
/// attribute, and the text parses. A line scanner skipping lines that start
/// with `#![` let an item written after the attribute on the same line through,
/// and text that does not parse is not known to declare nothing, so it counts
/// as source.
pub(super) fn declares_nothing(src: &str) -> bool {
    let Some(tree) = crate::make_parser().parse(src, None) else {
        return false;
    };
    let root = tree.root_node();
    if root.has_error() {
        return false;
    }
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .all(|n| matches!(n.kind(), "line_comment" | "block_comment" | "inner_attribute_item"))
}
