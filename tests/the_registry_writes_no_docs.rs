//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry writes no page into `docs/`.
//!
//! A registry edit that leaves a rendered page stale makes a landing that regenerates and
//! compares the worktree refuse, so the property that matters is that a registry edit changes
//! nothing under `docs/`. These arms pin the pages only: a table embedded into a template with
//! `{{registry:<key>}}` or `{{ ns }}` is rendered into the template's own document and is not
//! covered here.
//!
//! Every arm runs against a repository whose registry declares a `render = "page"` namespace,
//! because that is the shape that used to write `SPIKE.md`. The first arm fails when
//! `REGISTRY_PAGES_ENABLED` is set: `SPIKE.md` is written and the arm names it.

mod common;

use std::fs;
use std::path::Path;

use common::{Repo, write};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."
render = "page"

[[registry.namespace.field]]
name = "question"
type = "string"
required = true
description = "The question the spike answers."
"#;

fn row(question: &str) -> String {
    format!("[[spike]]\nid = \"first_one\"\nquestion = \"{question}\"\n")
}

/// The base repository of the harness, with a page namespace and one row on top of it.
fn with_registry() -> Repo {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(&r.root.join("mock/registry/spike.toml"), &row("a"));
    r
}

fn docs_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("docs"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A page namespace with a row renders no page, and the documents that are not the
/// registry's still render: the control that keeps "nothing was written" from being "nothing
/// ran".
#[test]
fn a_registry_change_renders_nothing_under_docs() {
    let r = with_registry();
    r.generate();
    let names = docs_names(&r.root);
    assert!(
        !names.iter().any(|n| n == "SPIKE.md"),
        "the registry wrote its page: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "ALPHA.md"),
        "generation of a document the registry does not own stopped too: {names:?}"
    );
}

/// The landing check, spelled as it is run: regenerate, then ask whether the worktree moved.
/// A registry row edited and regenerated leaves `docs/` exactly as committed.
#[test]
fn editing_a_registry_row_leaves_docs_unchanged_after_regeneration() {
    let r = with_registry();
    r.generate();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a registry"]);

    write(&r.root.join("mock/registry/spike.toml"), &row("a different question"));
    r.generate();

    let moved = r.git(&["status", "--porcelain", "--", "docs"]);
    assert!(moved.is_empty(), "regeneration changed docs/ after a registry edit:\n{moved}");
}

/// A page an earlier version wrote is retired by the sweep, so a consumer needs no hand
/// deletion for the pages to go.
#[test]
fn a_page_the_registry_wrote_before_is_swept() {
    let r = with_registry();
    write(
        &r.root.join("docs/SPIKE.md"),
        "<!--\n  AUTO-GENERATED: DO NOT EDIT DIRECTLY\n-->\n\n# Spikes\n",
    );
    r.generate();
    assert!(
        !r.root.join("docs/SPIKE.md").exists(),
        "a generated registry page outlived the registry writing pages: {:?}",
        docs_names(&r.root)
    );
}

/// A reference inside a row of a page namespace that resolves to nothing used to surface
/// through the page it was rendered into. With no page the row text reaches no document, so
/// generation reads the resolved rows themselves: the control is the same registry with a
/// reference that resolves, which passes.
#[test]
fn a_dangling_reference_in_a_page_namespace_row_still_fails_generation() {
    let r = with_registry();
    write(
        &r.root.join("mock/registry/spike.toml"),
        &row("see {{ spike::nosuch }}"),
    );
    let out = r
        .cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("--generate-only")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a row holding a reference to nothing generated cleanly:\n{stderr}"
    );
    assert!(
        stderr.contains("dangling-reference"),
        "the failure did not name the dangling reference:\n{stderr}"
    );
}

/// The control of the arm above: a reference that does resolve generates cleanly.
#[test]
fn a_resolving_reference_in_a_page_namespace_row_generates_cleanly() {
    let r = with_registry();
    write(
        &r.root.join("mock/registry/spike.toml"),
        &format!(
            "{}\n[[spike]]\nid = \"second_one\"\nquestion = \"see {{{{ spike::first_one::question }}}}\"\n",
            row("a")
        ),
    );
    r.generate();
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
