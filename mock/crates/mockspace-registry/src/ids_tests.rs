//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Path ids: which texts are one, where a reference's id ends, the anchor, and the branch finding.

use super::*;
use crate::test_world::{namespace, rows_from, rows_from_files};

fn tickets() -> Vec<RegistryNamespace> {
    vec![namespace("ticket", &[("title", "string", false), ("status", "string", false)], &[])]
}


#[test]
fn a_row_id_is_one_or_more_plain_names() {
    for good in ["a", "waist_a", "question::engine::allowed", "a__b::c_"] {
        assert!(is_valid_row_id(good), "{good}");
    }
    // A hyphen may stand in a reference's root, never in an id; nothing but plain names is an id.
    for bad in ["", "A", "2fast", "a-b::c", "a::b-c", "a::*", "a::{b, c}", "a::3", "a :: b", "a::b<x>", "f(a)"] {
        assert!(!is_valid_row_id(bad), "{bad:?} read as an id");
        assert!(row_id_names(bad).is_err(), "{bad:?}");
    }
    assert_eq!(row_id_names("question::engine").unwrap(), ["question", "engine"]);
}

#[test]
fn anchors_stay_apart_where_the_separator_falls_differently() {
    assert_eq!(row_anchor("waist_a"), "waist-a", "a one-name id keeps the anchor it always had");
    let pairs = [("a::b_c", "a_b::c"), ("a__b", "a::b"), ("a::b::c", "a_b_c"), ("a_::b", "a::_b")];
    for (one, other) in pairs {
        assert_ne!(row_anchor(one), row_anchor(other), "{one} and {other} anchored alike");
    }
    assert_eq!(row_anchor("question::engine_x::allowed"), "question.engine-x.allowed");
}

#[test]
fn an_id_that_is_a_proper_prefix_of_another_is_reported_with_both_files() {
    let short = "[[ticket]]\nid = \"question::engine\"\n";
    let long = "[[ticket]]\nid = \"question::engine::allowed\"\n";
    let reg = rows_from_files(&[("a.toml", short), ("b.toml", long)], &tickets());
    let found = id_branches(&reg);
    assert_eq!(found.len(), 1, "{found:?}");
    let f = &found[0];
    assert_eq!(f.kind, "id-is-a-branch");
    for part in ["ticket::question::engine ", "ticket::question::engine::allowed", "a.toml", "b.toml"] {
        assert!(f.message.contains(part), "`{part}` missing: {}", f.message);
    }
    assert!(validate(&tickets(), &reg).iter().any(|f| f.kind == "id-is-a-branch"), "the gate carries it");
}

#[test]
fn ids_sharing_text_but_no_path_prefix_are_not_branches() {
    // `question::engine_x` begins with the text `question::engine` and is no step below it, and
    // `question::engine::a` and `question::engine::b` are siblings.
    let rows = "[[ticket]]\nid = \"question::engine_x\"\n\n[[ticket]]\nid = \"question::engine::a\"\n\n\
                [[ticket]]\nid = \"question::engine::b\"\n\n[[ticket]]\nid = \"question::engine1\"\n";
    let reg = rows_from(rows, &tickets());
    assert!(id_branches(&reg).is_empty(), "{:?}", id_branches(&reg));
}

#[test]
fn the_same_id_in_two_namespaces_is_no_branch() {
    let nss = vec![namespace("ticket", &[], &[]), namespace("task", &[], &[])];
    let rows = "[[ticket]]\nid = \"a\"\n\n[[task]]\nid = \"a::b\"\n";
    assert!(id_branches(&rows_from(rows, &nss)).is_empty());
}

#[test]
fn the_prefix_pairs_are_a_pure_function_of_the_ids() {
    let ids = ["question::engine::allowed", "question::engine", "question::engine_x", "question", "other"];
    assert_eq!(
        id_prefix_pairs(&ids),
        [
            ("question", "question::engine"),
            ("question", "question::engine::allowed"),
            ("question", "question::engine_x"),
            ("question::engine", "question::engine::allowed"),
        ]
    );
    assert!(id_prefix_pairs(&["a::b", "a::c", "ab"]).is_empty());
}

#[test]
fn a_table_anchors_a_path_id_as_its_row_does() {
    let id = "question::engine_x::deps_allowed";
    let table = crate::Resolved::Table {
        namespace: "ticket".into(),
        columns:   vec!["id".into()],
        rows:      vec![vec![id.into()]],
    };
    let md = table.to_markdown();
    assert!(md.contains(&format!("<a id=\"{}\"></a>{id}", row_anchor(id))), "{md}");
    assert!(md.contains("question.engine-x.deps-allowed"), "{md}");
}

#[test]
fn the_registry_root_is_passed_over_by_one_function() {
    assert_eq!(names_after_root(&["reg", "ticket", "a"]), ["ticket", "a"]);
    assert_eq!(names_after_root(&["ticket", "a"]), ["ticket", "a"]);
    // `reg` alone names the root and nothing in it, and is kept as written.
    assert_eq!(names_after_root(&["reg"]), ["reg"]);
}

#[test]
fn the_branch_finding_says_its_text_through_one_function() {
    let short = "[[ticket]]\nid = \"q::e\"\n";
    let long = "[[ticket]]\nid = \"q::e::a\"\n";
    let reg = rows_from_files(&[("a.toml", short), ("b.toml", long)], &tickets());
    let found = id_branches(&reg);
    assert_eq!(
        found[0].message,
        id_is_a_branch_message(
            "ticket::q::e",
            &Path::new("reg").join("a.toml").display().to_string(),
            "ticket::q::e::a",
            &Path::new("reg").join("b.toml").display().to_string()
        )
    );
}
