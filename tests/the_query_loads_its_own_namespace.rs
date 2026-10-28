//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A query reads the namespaces it can reach and parses no file of any other.
//!
//! The observable is the loader's own warning for a file that is not valid TOML: a file the
//! loader never parses cannot warn. A query for one namespace stays silent about a broken file
//! of another, a query that reaches the other namespace still warns, and the answer for a
//! namespace is the same whether or not the others were read.

mod common;

use common::{Repo, write};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."

[[registry.namespace.field]]
name = "note"
type = "string"
description = "Free text, which may cite another namespace."

[[registry.namespace]]
key = "bulk"
title = "Bulk"
description = "A namespace the spike query has no business reading."

[[registry.namespace.field]]
name = "note"
type = "string"
description = "Free text."
"#;

const BROKEN: &str = "[[bulk]]\nid = \"never_parsed\"\nnote = \n";

fn repo(spike_note: &str) -> Repo {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(
        &r.root.join("mock/registry/spike.toml"),
        &format!("[[spike]]\nid = \"first_one\"\nnote = \"{spike_note}\"\n"),
    );
    write(&r.root.join("mock/registry/bulk.toml"), BROKEN);
    r
}

fn query(r: &Repo, expr: &str) -> (bool, String, String) {
    let out = r
        .cmd(common::BIN)
        .current_dir(r.root.join("mock"))
        .arg("query")
        .arg(expr)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const WARNS: &str = "is not valid TOML";

/// The case that must fail: the same broken file warns the moment a query names its namespace,
/// so the silence below is the file not being parsed, not a warning that never existed.
#[test]
fn a_query_for_the_broken_namespace_does_parse_its_file() {
    let r = repo("plain");
    let (_, _, err) = query(&r, "bulk");
    assert!(err.contains(WARNS), "the control did not warn:\n{err}");
}

#[test]
fn a_query_for_one_namespace_parses_no_file_of_another() {
    let r = repo("plain");
    let (ok, out, err) = query(&r, "spike");
    assert!(ok, "the query failed:\n{err}");
    assert!(out.contains("first_one"), "the row is missing:\n{out}");
    assert!(!err.contains(WARNS), "a file of another namespace was parsed:\n{err}");
}

/// A row text that cites another namespace pulls that namespace in, so the answer is the one a
/// full load would give: the cited file is read, and here that shows as the warning.
#[test]
fn a_cited_namespace_is_read_too() {
    let r = repo("see {{ bulk::never_parsed }}");
    let (_, _, err) = query(&r, "spike");
    assert!(err.contains(WARNS), "the cited namespace was not read:\n{err}");
}

#[test]
fn an_expression_that_asks_what_refers_to_a_row_reads_everything() {
    let r = repo("plain");
    let (_, _, err) = query(&r, "refsto(spike::first_one)");
    assert!(err.contains(WARNS), "refsto read only part of the registry:\n{err}");
}
