//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Selection over the id tree: each selector, the order and multiplicity, and what is refused.

use super::*;
use crate::test_world::{namespace, rows_from};

fn nss() -> Vec<RegistryNamespace> {
    vec![
        namespace("ticket", &[("title", "string", false)], &[]),
        namespace("task", &[], &[]),
        namespace("ikiuni-renderer", &[], &[]),
    ]
}

/// Written out of order, so an answer in byte order is the selection's and not the file's.
const ROWS: &str = r#"
[[ticket]]
id = "task::engine::internals::split"

[[ticket]]
id = "question::engine::render::pass"

[[ticket]]
id = "question::engine::internals::deps::allowed"

[[ticket]]
id = "question::tools::lint"

[[ticket]]
id = "question::engine::internals::cache"

[[ticket]]
id = "task::docs::readme"

[[ticket]]
id = "internals"

[[task]]
id = "unrelated"

[[ikiuni-renderer]]
id = "c_one"
"#;

fn select(query: &str) -> Result<Vec<String>, SelectRefusal> {
    let reg = rows_from(ROWS, &nss());
    reg.select(&nss(), query).map(|rows| rows.iter().map(|r| r.id.clone()).collect())
}

fn ok(query: &str) -> Vec<String> {
    select(query).unwrap_or_else(|r| panic!("{query}: {}", r.say(query)))
}

const ENGINE: [&str; 3] =
    ["question::engine::internals::cache", "question::engine::internals::deps::allowed", "question::engine::render::pass"];

#[test]
fn a_prefix_selects_every_row_below_it_in_byte_order() {
    assert_eq!(ok("ticket::question::engine"), ENGINE);
}

#[test]
fn a_trailing_deep_wildcard_selects_what_the_prefix_does() {
    assert_eq!(ok("ticket::question::engine::**"), ENGINE);
    assert_eq!(ok("reg::ticket::question::engine::**"), ENGINE, "the registry root is optional");
}

#[test]
fn a_deep_wildcard_in_the_middle_matches_zero_segments() {
    // `**` taken as nothing puts `question` right under the namespace.
    assert_eq!(ok("ticket::**::question::tools::lint"), ["question::tools::lint"]);
    // And `internals` as a whole id, at depth zero below the namespace.
    assert!(ok("ticket::**::internals").contains(&"internals".to_string()));
}

#[test]
fn a_deep_wildcard_in_the_middle_matches_several_segments() {
    assert_eq!(
        ok("ticket::**::internals::**"),
        [
            "internals",
            "question::engine::internals::cache",
            "question::engine::internals::deps::allowed",
            "task::engine::internals::split",
        ]
    );
}

#[test]
fn a_wildcard_selects_each_member_and_what_is_below_it() {
    assert_eq!(ok("ticket::question::engine::internals::*"), &ENGINE[.. 2]);
    assert_eq!(ok("ticket::*::engine::render::*"), ["question::engine::render::pass"]);
}

#[test]
fn a_set_keeps_its_written_order_across_members_with_steps_after_it() {
    assert_eq!(
        ok("ticket::{task, question}::engine::*"),
        [
            "task::engine::internals::split",
            "question::engine::internals::cache",
            "question::engine::internals::deps::allowed",
            "question::engine::render::pass",
        ]
    );
    // A member may carry steps of its own.
    assert_eq!(ok("ticket::{question::tools, task::docs}"), ["question::tools::lint", "task::docs::readme"]);
}

#[test]
fn a_plain_id_selects_its_row() {
    assert_eq!(ok("ticket::question::tools::lint"), ["question::tools::lint"]);
    assert_eq!(ok("task::unrelated"), ["unrelated"]);
    assert_eq!(ok("ikiuni-renderer::c_one"), ["c_one"], "a host's hyphen in the namespace");
}

#[test]
fn an_id_that_is_a_branch_selects_the_rows_under_it() {
    assert_eq!(ok("ticket::question::engine::internals"), &ENGINE[.. 2]);
}

#[test]
fn a_namespace_alone_selects_all_its_rows() {
    assert_eq!(ok("ticket").len(), 7);
    assert_eq!(ok("*").len(), 9, "every row of every namespace");
}

#[test]
fn each_row_comes_once_however_many_paths_reach_it() {
    assert_eq!(ok("ticket::{question::engine, question}"), [&ENGINE[..], &["question::tools::lint"]].concat());
    assert_eq!(ok("ticket::**::**").len(), 7);
}

#[test]
fn nothing_matched_is_an_empty_answer() {
    for q in ["ticket::question::nothing::**", "ticket::*::zzz", "ticket::question::eng"] {
        assert_eq!(select(q), Ok(Vec::new()), "{q}");
    }
}

#[test]
fn an_ordinal_or_a_range_is_refused() {
    let refused = select("ticket::question::3").unwrap_err();
    assert!(matches!(refused, SelectRefusal::Ordered(ref s) if s == "3"), "{refused:?}");
    assert!(refused.say("ticket::question::3").contains("position"));
    assert!(matches!(select("ticket::1..2"), Err(SelectRefusal::Ordered(_))));
}

#[test]
fn a_field_step_from_a_named_row_is_refused_and_after_a_wildcard_passes_over() {
    let refused = select("ticket::question::tools::lint::title").unwrap_err();
    assert_eq!(
        refused,
        SelectRefusal::FieldStep {
            row:  "ticket::question::tools::lint".into(),
            step: "title".into(),
        }
    );
    assert_eq!(select("ticket::**::title"), Ok(Vec::new()));
}

#[test]
fn other_queries_that_select_no_rows_are_refused() {
    assert!(matches!(select("tickets::**"), Err(SelectRefusal::UnknownNamespace(n)) if n == "tickets"));
    assert!(matches!(select("ticket::*<open>"), Err(SelectRefusal::Constraint(_))));
    assert_eq!(select("refsto(ticket::internals)"), Err(SelectRefusal::Operation));
    assert!(matches!(select("ticket::Question"), Err(SelectRefusal::Syntax(_))));
    assert!(matches!(select("ticket::a-b"), Err(SelectRefusal::Syntax(_))), "a hyphen past the namespace");
}
