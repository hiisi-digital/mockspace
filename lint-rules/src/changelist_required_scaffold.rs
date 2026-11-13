//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `changelist-required` passes outside IMPL: scaffolding, and a nuke.

use std::path::{Path, PathBuf};

use super::staged_or_worktree;
use crate::reading::Entry;
use crate::src_layout::SrcLayout;
use crate::View;

/// The marker a nuked crate's `lib.rs` stub carries, and the one place it is
/// spelled: the launcher's config defaults to it and `cargo mock --nuke`
/// writes it through [`nuke_stub`].
pub const NUKE_MARKER: &str = "Nuked by `cargo mock --nuke`";

/// The `lib.rs` a nuke leaves for crate `name`, carrying `marker`.
///
/// A proc-macro crate keeps `extern crate proc_macro;`, its one declaration,
/// so the stub still builds as a proc-macro crate.
pub fn nuke_stub(name: &str, proc_macro: bool, marker: &str) -> String {
    let head = format!(
        "//! {name}: {}.\n//!\n//! {marker}. Rewrite from design docs (mechanical, no reinterpretation).\n",
        if proc_macro { "proc macro crate" } else { "nuked" }
    );
    if proc_macro { format!("{head}\nextern crate proc_macro;\n") } else { head }
}

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
    view: &View<'_>,
    workspace_root: &Path,
    layout: &SrcLayout,
    file: &str,
    content: Option<&str>,
) -> bool {
    match content {
        None => crate_is_nuked(view, workspace_root, layout, file),
        Some(text) => {
            let stub_at_lib = is_stub(text)
                && lib_of(layout, file).is_some_and(|(src, lib)| {
                    lib == file && nothing_else_left(view, workspace_root, &src, &lib)
                });
            stub_at_lib || (declares_nothing(text) && nothing_at_head(view, workspace_root, file))
        },
    }
}

/// Whether `text` is the stub a nuke writes over `lib.rs`: the marker, and
/// nothing declared but a proc-macro crate's `extern crate proc_macro;`.
fn is_stub(text: &str) -> bool {
    text.contains(NUKE_MARKER)
        && top_level_only(text, |n, src| {
            n.kind() == "extern_crate_declaration"
                && n.utf8_text(src.as_bytes())
                    .is_ok_and(|t| t == "extern crate proc_macro;")
        })
}

/// The `src` directory and `lib.rs` path of the package holding `file`.
fn lib_of(layout: &SrcLayout, file: &str) -> Option<(PathBuf, String)> {
    let src = layout.package_dir(Path::new(""), file)?.join("src");
    let lib = src.join("lib.rs").to_string_lossy().replace('\\', "/");
    Some((src, lib))
}

/// Whether `HEAD` holds nothing that declares anything at `file`: the path is
/// not in `HEAD`, or the blob there declares nothing.
fn nothing_at_head(view: &View<'_>, workspace_root: &Path, file: &str) -> bool {
    let out = view.git_in(workspace_root, &["show", &format!("HEAD:./{file}")]);
    if !out.started() {
        return false;
    }
    if !out.ok() {
        // Not in HEAD, or no HEAD yet: a file this change creates.
        return true;
    }
    out.text().is_some_and(declares_nothing)
}

/// Whether the crate holding `file` is being nuked: what the commit carries as
/// its `lib.rs` is the nuke stub, and no other Rust file of its `src` is left.
///
/// The marker alone proves nothing, since writing it is itself a change the
/// scaffolding arm would judge. Only the whole shape is a nuke. The stub is
/// read from the index; a `lib.rs` changed past it in the working tree is a
/// changed file of its own and is judged on its own text.
fn crate_is_nuked(view: &View<'_>, workspace_root: &Path, layout: &SrcLayout, file: &str) -> bool {
    lib_of(layout, file).is_some_and(|(src, lib)| {
        staged_or_worktree(view, workspace_root, &lib, "staged").is_some_and(is_stub)
            && nothing_else_left(view, workspace_root, &src, &lib)
    })
}

/// Whether `src` holds no Rust file other than `lib`, in the index or on disk.
fn nothing_else_left(view: &View<'_>, workspace_root: &Path, src: &Path, lib: &str) -> bool {
    !other_source_in_index(view, workspace_root, src, lib)
        && !other_source_on_disk(view, &workspace_root.join(src))
}

/// Whether the index holds a Rust file under `src` other than `lib`, both
/// relative to `workspace_root`. A failing query proves nothing, so it answers yes.
fn other_source_in_index(view: &View<'_>, workspace_root: &Path, src: &Path, lib: &str) -> bool {
    let src = src.to_string_lossy();
    let out = view.git_in(workspace_root, &["ls-files", "-z", "--", &src]);
    if !out.ok() {
        return true;
    }
    out.stdout()
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(String::from_utf8_lossy)
        .any(|p| p.ends_with(".rs") && p != lib)
}

/// Whether the directory `src` holds, at any depth, a Rust file other than its `lib.rs`.
fn other_source_on_disk(view: &View<'_>, src: &Path) -> bool {
    let Some(listing) = view.list(src) else {
        return false;
    };
    listing.entries().iter().any(|e| {
        if Entry::is_dir(e) {
            walk_has_rust(view, &src.join(e.name()))
        } else {
            e.name().ends_with(".rs") && e.name() != "lib.rs"
        }
    })
}

/// Whether `dir` holds a Rust file at any depth.
fn walk_has_rust(view: &View<'_>, dir: &Path) -> bool {
    view.walk(dir, |_, _| false)
        .iter()
        .any(|p| p.extension().is_some_and(|x| x == "rs"))
}

/// Whether a Rust file declares nothing at all.
///
/// Read from the parse tree: every top-level node is a comment or an inner
/// attribute, and the text parses. A line scanner skipping lines that start
/// with `#![` let an item written after the attribute on the same line through,
/// and text that does not parse is not known to declare nothing, so it counts
/// as source.
pub(super) fn declares_nothing(src: &str) -> bool {
    top_level_only(src, |_, _| false)
}

/// Whether `src` parses and every top-level node is a comment, an inner
/// attribute, or a node `also` admits.
fn top_level_only(src: &str, also: impl Fn(tree_sitter::Node<'_>, &str) -> bool) -> bool {
    let Some(tree) = crate::make_parser().parse(src, None) else {
        return false;
    };
    let root = tree.root_node();
    if root.has_error() {
        return false;
    }
    let mut cursor = root.walk();
    root.children(&mut cursor).all(|n| {
        matches!(
            n.kind(),
            "line_comment" | "block_comment" | "inner_attribute_item"
        ) || also(n, src)
    })
}
