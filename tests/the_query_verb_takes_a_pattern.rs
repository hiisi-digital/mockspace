//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock query` takes a pattern over row ids, read by the same reader a document's
//! references are: the rows it selects, nothing on a wildcard matching nothing, and a failure with
//! the reason for a query that names nothing.

#[path = "common/ambient.rs"]
mod ambient;
mod common;

use common::{Repo, write};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work."
"#;

const ROWS: &str = r#"
[[ticket]]
id = "question::engine::render"

[[ticket]]
id = "question::engine::internals::deps"

[[ticket]]
id = "task::engine::internals::split"

[[ticket]]
id = "flat"
"#;

fn query(expr: &str) -> (Option<i32>, String, String) {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(&r.root.join("mock/registry/ticket.toml"), ROWS);
    let out = r.cmd(common::BIN).current_dir(r.root.join("mock")).arg("query").arg(expr).output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_branch_prints_the_rows_below_it() {
    let (code, out, err) = query("ticket::question::engine");
    assert_eq!(code, Some(0), "{err}");
    assert!(out.contains("question::engine::internals::deps") && out.contains("question::engine::render"), "{out}");
    assert!(!out.contains("task::") && !out.contains("flat"), "{out}");
}

#[test]
fn a_deep_wildcard_reaches_every_depth_and_counts() {
    let (code, out, err) = query("ticket::**::internals::**.count()");
    assert_eq!((code, out.as_str()), (Some(0), "2\n"), "{err}");
}

#[test]
fn a_wildcard_matching_nothing_is_an_empty_answer() {
    let (code, out, err) = query("ticket::*::idea.count()");
    assert_eq!((code, out.as_str()), (Some(0), "0\n"), "{err}");
}

#[test]
fn a_step_naming_nothing_fails_and_says_why() {
    let (code, out, err) = query("ticket::idea::**");
    assert_eq!(code, Some(1));
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("`idea` names nothing there"), "{err}");
}

#[test]
fn a_position_is_refused_with_the_reason() {
    let (code, _, err) = query("ticket::question::2");
    assert_eq!(code, Some(1));
    assert!(err.contains("position"), "{err}");
}
