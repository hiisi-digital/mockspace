//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use letkajenkka::Query;

use super::*;

/// What a host's name becomes in a reference, one row for each class a name falls in: case, the
/// hyphen, a name already folded, and each way no folding makes an identifier.
const TABLE: &[(&str, Option<&str>)] = &[
    ("DESIGN", Some("design")),
    ("my-crate", Some("my_crate")),
    ("No-Heap", Some("no_heap")),
    ("no_std", Some("no_std")),
    ("x2-Y3", Some("x2_y3")),
    ("2d_grid", None),
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
// the standard refuses, and folding `_` or a digit into an opening letter names `_private` and
// `2d_grid`; each fails a row.
#[test]
fn a_name_folds_into_the_standards_character_set_or_is_no_name() {
    for (name, want) in TABLE {
        let got = fold(name).map(|f| f.to_string());
        assert_eq!(got.as_deref(), *want, "{name:?}");
    }
}

// The fold agrees with the standard: a name folds to `Some` exactly where its case lowered and its
// hyphens made underscores is a path of one name the parser accepts, and that name is the fold.
//
// Mutation: a fold with a character set of its own (one admitting `.` or a leading `_`, say)
// disagrees with the parser on a row.
#[test]
fn a_name_folds_exactly_where_the_standard_reads_the_folded_text_as_one_name() {
    let mapped = |n: &str| {
        n.chars()
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
