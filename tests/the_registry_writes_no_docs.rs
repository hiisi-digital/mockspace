//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry writes no page into `docs/`, and what it still gives a document stays.
//!
//! A registry edit that leaves a rendered page stale makes a landing that regenerates and
//! compares the worktree refuse, so the property that matters is that a registry edit changes
//! nothing under `docs/` except through a template that reads the registry. Two surfaces are
//! kept and pinned beside the absence: a template reading a registry value, or a whole
//! namespace as a table, and a query result printed as a table.

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

[[registry.namespace.field]]
name = "question"
type = "string"
required = true
description = "The question the spike answers."
"#;

fn row(question: &str) -> String {
    format!("[[spike]]\nid = \"first_one\"\nquestion = \"{question}\"\n")
}

/// The base repository of the harness, with a namespace and one row on top of it.
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

fn query(r: &Repo, expr: &str) -> std::process::Output {
    r.cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("query")
        .arg(expr)
        .output()
        .unwrap()
}

/// A namespace with a row renders no page, and the documents that are not the registry's
/// still render: the control that keeps "nothing was written" from being "nothing ran".
#[test]
fn a_registry_change_renders_nothing_under_docs() {
    let r = with_registry();
    r.generate();
    let names = docs_names(&r.root);
    assert!(
        !names.iter().any(|n| n.ends_with("SPIKE.md")),
        "the registry wrote a page: {names:?}"
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

/// A registry value stated once in a template reaches the document it is written in. The
/// control is the same template after the row's value changes: the document follows it, so
/// the text is read from the registry and not carried by the template.
#[test]
fn a_template_reads_a_registry_value() {
    let r = with_registry();
    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "The question: {{ spike::first_one::question }}\n",
    );
    r.generate();
    assert!(r.doc().contains("The question: a\n"), "the value was not read:\n{}", r.doc());

    write(&r.root.join("mock/registry/spike.toml"), &row("a different question"));
    r.generate();
    assert!(
        r.doc().contains("The question: a different question\n"),
        "the document did not follow the registry:\n{}",
        r.doc()
    );
}

/// A whole namespace printed as a table inside a template, with the row's value in a cell.
#[test]
fn a_template_prints_a_namespace_as_a_table() {
    let r = with_registry();
    write(&r.root.join("mock/ALPHA.md.tmpl"), "{{ spike }}\n");
    r.generate();
    let doc = r.doc();
    assert!(doc.contains("| id | question |"), "no table header:\n{doc}");
    assert!(doc.contains("first_one"), "the row is missing from the table:\n{doc}");
    assert!(doc.contains("| a |"), "the row's value is missing from the table:\n{doc}");
}

/// The case that must fail: a template reading a row that does not exist is reported and
/// stops generation, rather than printing something that looks checked.
#[test]
fn a_template_reading_a_missing_row_fails_generation() {
    let r = with_registry();
    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "The question: {{ spike::nosuch::question }}\n",
    );
    let out = r
        .cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("--generate-only")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a reference to nothing generated cleanly:\n{stderr}");
    assert!(stderr.contains("dangling-reference"), "the failure was not named:\n{stderr}");
}

/// A query for a namespace prints its rows as a table on stdout.
#[test]
fn a_query_prints_a_namespace_as_a_table() {
    let r = with_registry();
    let out = query(&r, "spike");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the query failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let header = stdout.lines().next().unwrap_or("");
    assert!(
        header.starts_with("id") && header.contains("question"),
        "no table header:\n{stdout}"
    );
    assert!(stdout.contains("first_one"), "the row is missing:\n{stdout}");
    assert!(stdout.contains("1 rows in spike"), "no row count:\n{stdout}");
}

/// The case that must fail: a query that names no namespace prints no table and fails.
#[test]
fn a_query_for_nothing_prints_no_table() {
    let r = with_registry();
    let out = query(&r, "nosuch");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "a query for a missing namespace succeeded:\n{stdout}");
    assert!(!stdout.contains("rows in"), "a table was printed for nothing:\n{stdout}");
}

fn generate_output(r: &Repo) -> std::process::Output {
    r.cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("--generate-only")
        .output()
        .unwrap()
}

/// A reference inside a row that no template reads reaches no document, and `query` prints
/// it as it stands, so generation reads the resolved rows themselves.
#[test]
fn a_dangling_reference_in_an_unread_row_fails_generation() {
    let r = with_registry();
    write(&r.root.join("mock/registry/spike.toml"), &row("see {{ spike::nosuch }}"));
    let out = generate_output(&r);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a row holding a reference to nothing generated cleanly:\n{stderr}");
    assert!(stderr.contains("dangling-reference"), "the failure was not named:\n{stderr}");
}

/// The control of the arm above: a reference that does resolve generates cleanly.
#[test]
fn a_resolving_reference_in_a_row_generates_cleanly() {
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

/// The case that must fail for the table form: a template printing a namespace that is not
/// declared is reported rather than left as text that looks checked.
#[test]
fn a_template_printing_a_missing_namespace_fails_generation() {
    let r = with_registry();
    write(&r.root.join("mock/ALPHA.md.tmpl"), "{{ reg::nosuch }}\n");
    let out = generate_output(&r);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a table of nothing generated cleanly:\n{stderr}");
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
