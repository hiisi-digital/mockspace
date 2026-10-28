//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry renders no document, and nothing refuses a commit over one.
//!
//! The lead designer ruled on 2026-10-28 that the registry writes no generated pages until a
//! sensible way for that to work is settled. A registry edit that left `docs/TICKET-MOVE.md`
//! stale made every later landing refuse with "left the worktree changed", so the property
//! that matters is that a registry edit changes nothing under `docs/`, on the path a hook runs.
//!
//! Every arm runs against a repository whose registry declares a `render = "page"` namespace,
//! because that is the shape that used to write `SPIKE.md`. **The first arm is the one that
//! fails when the switch is on**: with `REGISTRY_PAGES_ENABLED` set, `SPIKE.md` is written and
//! the arm names it.

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

fn taplo_present() -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|d| !d.is_empty() && Path::new(d).join("taplo").exists())
}

/// The commit gate refuses nothing over a registry page, however stale one would be. A page
/// sits in `docs/` as an earlier version wrote it, the row it listed has changed, and the gate
/// on the path the pre-commit hook invokes passes.
#[test]
fn the_commit_gate_does_not_refuse_a_commit_whose_registry_pages_would_differ() {
    assert!(
        taplo_present(),
        "taplo absent: the schema check reports unavailable and fails every registry gate, \
         for a reason that has nothing to do with pages. Install taplo to run this suite."
    );
    let r = with_registry();
    write(
        &r.root.join("docs/SPIKE.md"),
        "<!--\n  AUTO-GENERATED: DO NOT EDIT DIRECTLY\n-->\n\n# Spikes\n\n| a | stale |\n",
    );
    write(&r.root.join("mock/registry/spike.toml"), &row("changed since the page was written"));
    let out = r
        .cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .args(["--lint-only", "--commit", "--scope", "infra"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "the gate refused a commit over a registry page:\n{}",
        Repo::failure(&out)
    );
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
