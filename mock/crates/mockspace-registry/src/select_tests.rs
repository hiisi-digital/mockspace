//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Selection over the id tree: each selector, the order and multiplicity, field steps, and what
//! fails.

use super::*;
use crate::test_world::{namespace, rows_from};

fn nss() -> Vec<RegistryNamespace> {
    vec![
        namespace("ticket", &[("title", "string", false)], &[]),
        namespace("task", &[], &[]),
        namespace("ikiuni-renderer", &[], &[]),
        namespace("t", &[], &[]),
    ]
}

/// Written out of order, so an answer in the standard's order is the selection's and not the file's.
const ROWS: &str = r#"
[[ticket]]
id = "task::engine::internals::split"

[[ticket]]
id = "question::engine::render::pass"

[[ticket]]
id = "question::engine::internals::deps::allowed"

[[ticket]]
id = "question::tools::lint"
title = "Lint the tools"

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

[[t]]
id = "a0"

[[t]]
id = "a::b"
"#;

fn select(query: &str) -> Result<Vec<String>, SelectRefusal> {
    let reg = rows_from(ROWS, &nss());
    reg.select(&nss(), query).map(|hits| {
        hits.iter()
            .map(|h| {
                match h {
                    Selected::Row(r) => r.id.clone(),
                    Selected::Field(r, f) => format!("{}.{f}", r.id),
                }
            })
            .collect()
    })
}

fn ok(query: &str) -> Vec<String> {
    select(query).unwrap_or_else(|r| panic!("{query}: {}", r.say(query)))
}

const ENGINE: [&str; 3] =
    ["question::engine::internals::cache", "question::engine::internals::deps::allowed", "question::engine::render::pass"];

const TICKETS: [&str; 7] = [
    "internals",
    "question::engine::internals::cache",
    "question::engine::internals::deps::allowed",
    "question::engine::render::pass",
    "question::tools::lint",
    "task::docs::readme",
    "task::engine::internals::split",
];

#[test]
fn a_prefix_selects_every_row_below_it() {
    assert_eq!(ok("ticket::question::engine"), ENGINE);
}

#[test]
fn a_trailing_deep_wildcard_selects_what_the_prefix_does() {
    assert_eq!(ok("ticket::question::engine::**"), ENGINE);
    assert_eq!(ok("reg::ticket::question::engine::**"), ENGINE, "the registry root is optional");
}

#[test]
fn members_come_in_byte_order_step_by_step_not_by_the_joined_id() {
    // `a` is before `a0`, so `a::b` comes first, though the text `a0` sorts before `a::b`.
    assert_eq!(ok("t::**"), ["a::b", "a0"]);
    assert_eq!(ok("t::*"), ["a::b", "a0"]);
}

#[test]
fn a_deep_wildcard_in_the_middle_matches_zero_segments() {
    assert_eq!(ok("ticket::**::question::tools::lint"), ["question::tools::lint"]);
    assert_eq!(ok("ticket::**::render::pass"), ["question::engine::render::pass"]);
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
    assert_eq!(ok("ticket::{question::tools, task::docs}"), ["question::tools::lint", "task::docs::readme"]);
}

#[test]
fn a_plain_id_selects_its_row() {
    assert_eq!(ok("ticket::question::tools::lint"), ["question::tools::lint"]);
    assert_eq!(ok("task::unrelated"), ["unrelated"]);
}

#[test]
fn a_namespace_is_named_folded_and_its_host_spelling_is_refused() {
    assert_eq!(ok("ikiuni_renderer::c_one"), ["c_one"]);
    assert!(matches!(select("ikiuni-renderer::c_one"), Err(SelectRefusal::Syntax(_))));
}

#[test]
fn two_namespaces_folding_to_one_name_are_refused_and_reported() {
    let nss = vec![namespace("ikiuni-renderer", &[], &[]), namespace("ikiuni_renderer", &[], &[])];
    let reg = rows_from("[[ikiuni-renderer]]\nid = \"a\"\n\n[[ikiuni_renderer]]\nid = \"b\"\n", &nss);
    let refused = reg.select(&nss, "ikiuni_renderer::*").unwrap_err();
    assert!(matches!(refused, SelectRefusal::Ambiguous(ref n, ref keys) if n == "ikiuni_renderer" && keys.len() == 2), "{refused:?}");
    let found = namespace_fold_collisions(&nss);
    assert_eq!(found.iter().map(|f| f.kind).collect::<Vec<_>>(), ["namespace-fold-collision"]);
    assert!(namespace_fold_collisions(&self::nss()).is_empty());
}

#[test]
fn an_id_that_is_a_branch_selects_the_rows_under_it() {
    assert_eq!(ok("ticket::question::engine::internals"), &ENGINE[.. 2]);
}

#[test]
fn a_namespace_alone_selects_all_its_rows() {
    assert_eq!(ok("ticket"), TICKETS);
    assert_eq!(ok("{task, t}"), ["unrelated", "a::b", "a0"]);
}

#[test]
fn each_row_comes_once_however_many_paths_reach_it() {
    assert_eq!(ok("ticket::{question::engine, question}"), [&ENGINE[..], &["question::tools::lint"]].concat());
    assert_eq!(ok("ticket::**::**"), TICKETS);
}

#[test]
fn a_wildcard_matching_nothing_is_an_empty_answer() {
    for q in ["ticket::*::zzz", "ticket::**::zzz", "ticket::*::engine::nothing::**"] {
        assert_eq!(select(q), Ok(Vec::new()), "{q}");
    }
}

#[test]
fn a_step_naming_nothing_where_nothing_widened_fails() {
    for (q, step) in [
        ("ticket::question::nothing::**", "nothing"),
        ("ticket::question::eng", "eng"),
        ("tickets::**", "tickets"),
        ("ticket::question::tools::lint::nothing", "nothing"),
    ] {
        assert_eq!(select(q), Err(SelectRefusal::NoSuch(step.into())), "{q}");
    }
    assert!(SelectRefusal::NoSuch("eng".into()).say("q").contains("no step before it widened"));
}

#[test]
fn a_step_from_a_row_names_a_declared_field() {
    assert_eq!(ok("ticket::question::tools::lint::title"), ["question::tools::lint.title"]);
    assert_eq!(ok("ticket::question::tools::lint::*"), ["question::tools::lint.id", "question::tools::lint.title"]);
    // After a wildcard every row of the namespace has the field, as the namespace is uniform.
    assert_eq!(ok("ticket::question::*::lint::title"), ["question::tools::lint.title"]);
    // Where the namespace declares no such field the row is passed over, and nothing fails.
    assert_eq!(ok("{ticket, task}::*::title"), ["internals.title"]);
}

#[test]
fn a_step_past_a_field_is_refused() {
    assert_eq!(
        select("ticket::question::tools::lint::title::x"),
        Err(SelectRefusal::PastField {
            row:   "ticket::question::tools::lint".into(),
            field: "title".into(),
        })
    );
}

#[test]
fn an_ordinal_or_a_range_is_refused() {
    let refused = select("ticket::question::3").unwrap_err();
    assert!(matches!(refused, SelectRefusal::Ordered(ref s) if s == "3"), "{refused:?}");
    assert!(refused.say("ticket::question::3").contains("position"));
    assert!(matches!(select("ticket::1..2"), Err(SelectRefusal::Ordered(_))));
}

#[test]
fn a_constraint_an_operation_and_bad_syntax_are_refused() {
    assert!(matches!(select("ticket::*<open>"), Err(SelectRefusal::Constraint(_))));
    assert_eq!(select("refsto(ticket::internals)"), Err(SelectRefusal::Operation));
    assert!(matches!(select("ticket::Question"), Err(SelectRefusal::Syntax(_))));
}

#[test]
fn select_ids_is_the_same_reader_over_a_plain_list() {
    let ids = ["ticket::question::engine::b", "ticket::question::engine::a", "ticket::task::c", "not an id", "task::x"];
    assert_eq!(select_ids("ticket::question::**", &ids).unwrap(), ["ticket::question::engine::a", "ticket::question::engine::b"]);
    assert_eq!(select_ids("ticket::*::c", &ids).unwrap(), ["ticket::task::c"]);
    assert_eq!(select_ids("ticket::**::zzz", &ids).unwrap(), Vec::<&str>::new());
    assert_eq!(select_ids("ticket::idea", &ids), Err(SelectRefusal::NoSuch("idea".into())));
    // No field is declared over a plain list, so a step from a row fails where nothing widened.
    assert_eq!(select_ids("ticket::task::c::title", &ids), Err(SelectRefusal::NoSuch("title".into())));
}
