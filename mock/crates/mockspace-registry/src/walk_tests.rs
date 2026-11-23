//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The walker as a plain read sees it: where an id ends, a field after it, a reference field gone
//! on from, and a set of one read as its member.

use crate::test_world::{namespace, rows_from};
use crate::*;

fn nss() -> Vec<RegistryNamespace> {
    vec![
        namespace(
            "ticket",
            &[
                ("title", "string", false),
                ("needs", "ticket[]", false),
                ("parent", "ticket", false),
                ("related", "ref[]", false),
            ],
            &[],
        ),
        namespace("task", &[("title", "string", false)], &[]),
    ]
}

const ROWS: &str = r#"
[[ticket]]
id = "question::engine::a"
title = "A"
needs = ["task::engine::b", "task::engine::c"]
related = ["task::lone", "research::notes::x"]

[[ticket]]
id = "task::engine::b"
title = "B"
parent = "question::engine::a"

[[ticket]]
id = "task::engine::c"
title = "C"

[[ticket]]
id = "lonely"

[[task]]
id = "lone"
title = "Lone"
"#;

fn read(query: &str) -> Result<(Vec<String>, bool), SelectRefusal> {
    let reg = rows_from(ROWS, &nss());
    reg.select(&nss(), query).map(|found| {
        let items = found
            .items
            .iter()
            .map(|s| {
                match s {
                    Selected::Row(r) => r.qualified(),
                    Selected::Field(r, f) => format!("{}.{f}", r.qualified()),
                }
            })
            .collect();
        (items, found.single)
    })
}

fn one(query: &str) -> String {
    match read(query) {
        Ok((items, true)) if items.len() == 1 => items[0].clone(),
        other => panic!("{query} read as {other:?}, not one member"),
    }
}

#[test]
fn a_path_id_and_the_field_after_it() {
    assert_eq!(one("ticket::question::engine::a"), "ticket::question::engine::a");
    assert_eq!(one("ticket::question::engine::a::title"), "ticket::question::engine::a.title");
    assert_eq!(one("reg::ticket::task::engine::b::id"), "ticket::task::engine::b.id");
}

#[test]
fn a_one_name_id_beside_a_path_id() {
    assert_eq!(one("ticket::lonely"), "ticket::lonely");
    assert_eq!(one("ticket::lonely::title"), "ticket::lonely.title", "a declared field the row does not carry");
}

#[test]
fn a_set_of_one_reads_as_its_member() {
    assert_eq!(one("ticket::{task}::engine::b::title"), "ticket::task::engine::b.title");
    assert_eq!(one("ticket::{task::engine::b}"), "ticket::task::engine::b");
    let (items, single) = read("ticket::{question}::engine").unwrap();
    assert_eq!((items.as_slice(), single), (&["ticket::question::engine::a".to_string()][..], false), "a branch is no single row");
}

#[test]
fn the_longer_id_wins_a_name_where_one_id_is_a_prefix_of_another() {
    let nss = vec![namespace("n", &[("title", "string", false), ("b", "string", false)], &[])];
    let reg = rows_from("[[n]]\nid = \"a\"\nb = \"field\"\n\n[[n]]\nid = \"a::b\"\n", &nss);
    let ids = |q: &str| -> Vec<String> {
        reg.select(&nss, q)
            .unwrap()
            .items
            .iter()
            .map(|s| {
                match s {
                    Selected::Row(r) => r.qualified(),
                    Selected::Field(r, f) => format!("{}.{f}", r.qualified()),
                }
            })
            .collect()
    };
    assert_eq!(ids("n::a::b"), ["n::a::b"], "the row `a::b`, not the field `b` of `a`");
    assert_eq!(ids("n::a::title"), ["n::a.title"], "a name no longer id takes is a field of `a`");
}

#[test]
fn a_step_past_a_reference_field_goes_on_into_the_rows_it_names() {
    assert_eq!(
        read("ticket::question::engine::a::needs::title").unwrap().0,
        ["ticket::task::engine::b.title", "ticket::task::engine::c.title"]
    );
    assert_eq!(one("ticket::task::engine::b::parent::title"), "ticket::question::engine::a.title");
    assert_eq!(read("ticket::*::**::needs::title").unwrap().0, ["ticket::task::engine::b.title", "ticket::task::engine::c.title"]);
    // A link in a `ref[]` field is a row too; a citation beside it names none and is passed over.
    assert_eq!(read("ticket::question::engine::a::related::title").unwrap().0, ["task::lone.title"]);
}

#[test]
fn a_step_past_a_value_field_fails_naming_the_member_and_its_type() {
    let refused = read("ticket::question::engine::a::title::x").unwrap_err();
    assert_eq!(
        refused,
        SelectRefusal::PastField {
            row:   "ticket::question::engine::a".into(),
            field: "title".into(),
            ty:    "string".into(),
        }
    );
    assert!(refused.say("q").contains("`title` of ticket::question::engine::a is a `string`"), "{}", refused.say("q"));
    // After a wildcard such a row is passed over.
    assert_eq!(read("ticket::**::title::x").unwrap().0, Vec::<String>::new());
}

#[test]
fn deep_from_a_record_covers_its_fields() {
    let (items, _) = read("ticket::task::engine::c::**").unwrap();
    assert_eq!(items[0], "ticket::task::engine::c");
    for f in ["id", "needs", "parent", "related", "title"] {
        assert!(items.contains(&format!("ticket::task::engine::c.{f}")), "{f}: {items:?}");
    }
    assert_eq!(read("ticket::task::engine::c::*").unwrap().0.len(), 5);
}
