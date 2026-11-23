//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use letkajenkka::Query;

use super::*;

/// What a host's name becomes in a reference, one row for each class a name falls in: case, the
/// hyphen, a name opening on a digit, a name already folded, and each way no folding makes an identifier.
const TABLE: &[(&str, Option<&str>)] = &[
    ("DESIGN", Some("design")),
    ("my-crate", Some("my_crate")),
    ("No-Heap", Some("no_heap")),
    ("no_std", Some("no_std")),
    ("x2-Y3", Some("x2_y3")),
    ("2d_grid", Some("t2d_grid")),
    ("202611011713_renderer-roadmap", Some("t202611011713_renderer_roadmap")),
    ("1", Some("t1")),
    ("_private", None),
    ("-lead", None),
    ("a.b", None),
    ("a b", None),
    ("a::b", None),
    ("", None),
    ("ärt", None),
    ("tab\there", None),
];

// Mutation: dropping the hyphen fold makes `my-crate` no name, keeping the case makes `DESIGN` one
// the standard refuses, leaving a digit unled makes `2d_grid` no name, and folding `_` into an
// opening letter names `_private`; each fails a row.
#[test]
fn a_name_folds_into_the_standards_character_set_or_is_no_name() {
    for (name, want) in TABLE {
        let got = fold(name).map(|f| f.to_string());
        assert_eq!(got.as_deref(), *want, "{name:?}");
    }
}

// The fold agrees with the standard: a name folds to `Some` exactly where its case lowered, its
// hyphens made underscores and a digit it opens on led is a path of one name the parser accepts,
// and that name is the fold.
//
// Mutation: a fold with a character set of its own (one admitting `.` or a leading `_`, say)
// disagrees with the parser on a row.
#[test]
fn a_name_folds_exactly_where_the_standard_reads_the_folded_text_as_one_name() {
    let mapped = |n: &str| {
        let lead = if n.starts_with(|c: char| c.is_ascii_digit()) { "t" } else { "" };
        lead.to_string()
            + &n.chars()
                .map(|c| if c == '-' { '_' } else { c.to_ascii_lowercase() })
                .collect::<String>()
    };
    for (name, _) in TABLE {
        let text = mapped(name);
        let standard = Query::parse(&text)
            .ok()
            .and_then(Query::path)
            .and_then(plain)
            .and_then(|mut names| {
                let one = names.next()?;
                names.next().is_none().then(|| one.to_string())
            });
        assert_eq!(fold(name).map(|f| f.to_string()), standard, "{name:?}");
    }
}

// Mutation: comparing the name as written, or folding the written side as well, makes `No-Heap`
// equal to itself and not to `no_heap`.
#[test]
fn a_folded_name_equals_the_written_name_it_folds_to_and_nothing_else() {
    let f = fold("No-Heap").unwrap();
    assert!(f == "no_heap");
    assert!(f != "No-Heap");
    assert!(f != "no_hea");
    assert!(f != "no_heaps");
}

fn names_of(text: &str) -> Option<Vec<&str>> {
    plain(Query::parse(text).ok()?.path()?).map(Iterator::collect)
}

// Mutation: yielding the names of the steps that are names and skipping the rest gives `foo` for
// `foo::*`, and ignoring constraints reads `foo::bar<x>` as a path of names.
#[test]
fn a_path_is_plain_only_where_every_step_is_a_name_with_no_constraint() {
    assert_eq!(names_of("foo::bar::baz"), Some(vec!["foo", "bar", "baz"]));
    assert_eq!(names_of("foo"), Some(vec!["foo"]));
    for text in [
        "foo::{a, b}",
        "foo::*",
        "foo::**",
        "foo::bar<x>",
        "foo::3",
        "foo::1..2",
        "*::foo",
    ] {
        assert_eq!(names_of(text), None, "{text}");
    }
}

// A name opening on a digit is cited with the lead before it, so a stamped research file is reached
// by `t202611011713_renderer_roadmap` and by nothing shorter or unled.
//
// Mutation: comparing the written name from its first byte, or dropping the lead in `Display`,
// makes the unled spelling equal and the led one not.
#[test]
fn a_name_opening_on_a_digit_equals_its_led_spelling_and_nothing_else() {
    let f = fold("202611011713_renderer-roadmap").unwrap();
    assert!(f == "t202611011713_renderer_roadmap");
    assert!(f != "202611011713_renderer_roadmap");
    assert!(f != "t202611011713_renderer_roadma");
    assert!(f != "u202611011713_renderer_roadmap");
    assert!(f != "t");
    assert_eq!(f.to_string(), "t202611011713_renderer_roadmap");
}

// A name of the host's own that folds to the lead and a stamp is the spelling a stamped name takes,
// so the two would answer one citation; only such a name is said to take it.
//
// Mutation: asking only of a lowercase `t` misses `T202611011713-x`; asking of any digits after the
// lead refuses `t3_timing_probe`; asking of the led name itself refuses every stamped file.
#[test]
fn only_a_name_whose_fold_opens_on_the_lead_and_a_stamp_takes_a_led_stamp() {
    for (name, takes) in [
        ("t202611011713_x", true),
        ("T202611011713-x", true),
        ("t202611011713", true),
        ("t2026110117130_x", false),
        ("t20261101171_x", false),
        ("t3_timing_probe", false),
        ("tests", false),
        ("t", false),
        ("202611011713_x", false),
        ("x202611011713", false),
    ] {
        assert_eq!(fold(name).unwrap().takes_a_led_stamp(), takes, "{name:?}");
    }
}

// What a directory entry is called: a directory by its whole name, a file up to its first dot.
//
// Mutation: folding a directory's name up to its first dot, as a file's is, names `v1.2` as `v1`;
// folding a file's whole name keeps the extension a reference never writes.
#[test]
fn an_entry_is_named_by_its_whole_name_or_a_file_up_to_its_first_dot() {
    let named = |name: &str, is_dir: bool| entry(name, is_dir).map(|f| f.to_string());
    assert_eq!(named("DESIGN.md.tmpl", false).as_deref(), Some("design"));
    assert_eq!(named("202611011713_a-b.md", false).as_deref(), Some("t202611011713_a_b"));
    assert_eq!(named("my-crate", true).as_deref(), Some("my_crate"));
    assert_eq!(named("v1.2", true), None);
    assert_eq!(named(".hidden", false), None);
}
