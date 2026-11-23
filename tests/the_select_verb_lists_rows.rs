//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock select` prints one qualified id a line, nothing on no match, and refuses a query
//! that selects no rows with a reason.

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

fn select(query: &str) -> (Option<i32>, String, String) {
    let r = Repo::new();
    write(&r.root.join("mock/mockspace.toml"), CONFIG);
    write(&r.root.join("mock/registry/ticket.toml"), ROWS);
    let out = r.cmd(common::BIN).current_dir(r.root.join("mock")).arg("select").arg(query).output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_prefix_prints_one_qualified_id_a_line() {
    let (code, out, err) = select("ticket::question::engine");
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, "ticket::question::engine::internals::deps\nticket::question::engine::render\n");
}

#[test]
fn a_deep_wildcard_reaches_every_depth() {
    let (code, out, err) = select("ticket::**::internals::**");
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, "ticket::question::engine::internals::deps\nticket::task::engine::internals::split\n");
}

#[test]
fn no_match_is_silent_and_succeeds() {
    let (code, out, err) = select("ticket::idea::**");
    assert_eq!((code, out.as_str()), (Some(0), ""), "{err}");
    assert!(!err.contains("idea"), "a pattern matching nothing is an answer, not an error: {err}");
}

#[test]
fn a_refused_query_says_why_and_fails() {
    let (code, out, err) = select("ticket::question::2");
    assert_eq!(code, Some(1));
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("position"), "{err}");
}
