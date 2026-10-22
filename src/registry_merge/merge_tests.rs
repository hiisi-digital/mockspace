//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::super::merge_text;
use super::super::scan::{Doc, Key};

fn row(id: &str, text: &str) -> String {
    format!("[[probe]]\nid = \"{id}\"\ntext = \"{text}\"\n")
}

fn file(rows: &[String]) -> String {
    rows.join("\n")
}

fn ids(text: &str) -> Vec<String> {
    Doc::parse(text)
        .unwrap()
        .keys()
        .iter()
        .map(|k| {
            match k {
                Key::Row {
                    id,
                    ..
                } => id.clone(),
                Key::Table(t) => format!("[{t}]"),
            }
        })
        .collect()
}

fn clean(base: &str, ours: &str, theirs: &str) -> String {
    let out = merge_text(base, ours, theirs, 7).unwrap();
    assert_eq!(out.conflicts, 0, "{}", out.text);
    out.text
}

fn conflicted(base: &str, ours: &str, theirs: &str) -> String {
    let out = merge_text(base, ours, theirs, 7).unwrap();
    assert!(out.conflicts > 0, "expected a conflict: {}", out.text);
    out.text
}

fn base() -> String {
    file(&[row("a", "1"), row("b", "2"), row("c", "3")])
}

#[test]
fn rows_added_at_the_same_spot_on_both_sides_are_both_kept() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("ours", "x")]);
    let t = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("theirs", "y")]);
    let merged = clean(&b, &o, &t);
    assert_eq!(ids(&merged), ["a", "b", "c", "ours", "theirs"]);
    assert!(!merged.contains("<<<<"), "{merged}");
    assert!(merged.contains(&row("ours", "x")) && merged.contains(&row("theirs", "y")));
}

#[test]
fn rows_added_in_the_middle_land_after_the_row_they_followed() {
    let b = base();
    let o = file(&[row("a", "1"), row("o1", "x"), row("o2", "x"), row("b", "2"), row("c", "3")]);
    let t = file(&[row("a", "1"), row("b", "2"), row("t1", "y"), row("c", "3")]);
    assert_eq!(ids(&clean(&b, &o, &t)), ["a", "o1", "o2", "b", "t1", "c"]);
}

#[test]
fn each_sides_own_order_is_kept_for_what_it_added() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("z2", ""), row("z1", "")]);
    let t = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("y2", ""), row("y1", "")]);
    assert_eq!(ids(&clean(&b, &o, &t)), [
        "a", "b", "c", "y2", "y1", "z2", "z1"
    ]);
}

#[test]
fn the_result_does_not_depend_on_which_side_is_ours() {
    let b = base();
    let o = file(&[row("a", "1"), row("m", "x"), row("b", "2"), row("c", "3"), row("n", "x")]);
    let t = file(&[row("a", "1"), row("k", "y"), row("b", "2"), row("c", "3"), row("p", "y")]);
    let one = clean(&b, &o, &t);
    let two = clean(&b, &t, &o);
    assert_eq!(ids(&one), ["a", "k", "m", "b", "c", "n", "p"]);
    assert_eq!(one, two);
}

#[test]
fn the_same_row_added_identically_on_both_sides_is_kept_once() {
    let b = base();
    let added = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("new", "same")]);
    let merged = clean(&b, &added, &added);
    assert_eq!(ids(&merged), ["a", "b", "c", "new"]);
    assert_eq!(merged, added);
}

#[test]
fn a_row_changed_on_one_side_only_takes_that_change() {
    let b = base();
    let changed = file(&[row("a", "1"), row("b", "CHANGED"), row("c", "3")]);
    assert_eq!(clean(&b, &changed, &b), changed);
    assert_eq!(clean(&b, &b, &changed), changed);
}

#[test]
fn changes_to_different_rows_on_each_side_both_apply() {
    let b = base();
    let o = file(&[row("a", "ours"), row("b", "2"), row("c", "3")]);
    let t = file(&[row("a", "1"), row("b", "2"), row("c", "theirs")]);
    assert_eq!(
        clean(&b, &o, &t),
        file(&[row("a", "ours"), row("b", "2"), row("c", "theirs")])
    );
}

#[test]
fn a_row_deleted_on_one_side_and_untouched_on_the_other_is_deleted() {
    let b = base();
    let gone = file(&[row("a", "1"), row("c", "3")]);
    assert_eq!(ids(&clean(&b, &gone, &b)), ["a", "c"]);
    assert_eq!(ids(&clean(&b, &b, &gone)), ["a", "c"]);
    assert_eq!(clean(&b, &gone, &b), gone);
}

#[test]
fn a_row_deleted_on_both_sides_stays_deleted() {
    let b = base();
    let gone = file(&[row("a", "1"), row("c", "3")]);
    assert_eq!(clean(&b, &gone, &gone), gone);
}

#[test]
fn a_row_changed_differently_on_both_sides_conflicts_around_that_row_alone() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "from ours"), row("c", "3")]);
    let t = file(&[row("a", "1"), row("b", "from theirs"), row("c", "3")]);
    let merged = conflicted(&b, &o, &t);
    let out = merge_text(&b, &o, &t, 7).unwrap();
    assert_eq!(out.conflicts, 1);
    let expect = format!(
        "{}\n\n<<<<<<< ours\n{}=======\n{}>>>>>>> theirs\n\n{}",
        row("a", "1").trim_end(),
        row("b", "from ours").trim_end().to_string() + "\n",
        row("b", "from theirs").trim_end().to_string() + "\n",
        row("c", "3")
    );
    assert_eq!(merged, expect);
}

#[test]
fn a_row_changed_on_one_side_and_deleted_on_the_other_conflicts() {
    let b = base();
    let changed = file(&[row("a", "1"), row("b", "CHANGED"), row("c", "3")]);
    let gone = file(&[row("a", "1"), row("c", "3")]);
    let merged = conflicted(&b, &changed, &gone);
    assert!(
        merged.contains(
            "<<<<<<< ours\n[[probe]]\nid = \"b\"\ntext = \"CHANGED\"\n=======\n>>>>>>> theirs"
        ),
        "{merged}"
    );
    let merged = conflicted(&b, &gone, &changed);
    assert!(
        merged.contains(
            "<<<<<<< ours\n=======\n[[probe]]\nid = \"b\"\ntext = \"CHANGED\"\n>>>>>>> theirs"
        ),
        "{merged}"
    );
}

#[test]
fn the_same_id_added_with_different_content_conflicts() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("new", "mine")]);
    let t = file(&[row("a", "1"), row("b", "2"), row("c", "3"), row("new", "yours")]);
    let merged = conflicted(&b, &o, &t);
    assert!(
        merged.contains("text = \"mine\"") && merged.contains("text = \"yours\""),
        "{merged}"
    );
}

#[test]
fn a_conflict_leaves_every_other_row_merged() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "x"), row("c", "3"), row("only-ours", "o")]);
    let t = file(&[row("a", "1"), row("b", "y"), row("c", "3"), row("only-theirs", "t")]);
    let merged = conflicted(&b, &o, &t);
    assert!(
        merged.contains(&row("only-ours", "o")) && merged.contains(&row("only-theirs", "t")),
        "{merged}"
    );
    assert_eq!(merged.matches("<<<<<<<").count(), 1);
}

#[test]
fn the_marker_length_is_the_one_git_asked_for() {
    let b = base();
    let o = file(&[row("a", "1"), row("b", "x"), row("c", "3")]);
    let t = file(&[row("a", "1"), row("b", "y"), row("c", "3")]);
    let out = merge_text(&b, &o, &t, 9).unwrap();
    assert!(
        out.text.contains("<<<<<<<<< ours\n") && out.text.contains("\n=========\n"),
        "{}",
        out.text
    );
}

#[test]
fn comments_and_formatting_of_untouched_rows_survive_byte_for_byte() {
    let odd = "# why a exists\n[[probe]]\nid   =   \"a\"   # trailing\ntext='single'\nlist = [\n  1,\n  2,\n]\n";
    let b = format!("# file header\n\n{odd}\n{}", row("b", "2"));
    let o = format!("{b}\n# about o\n{}", row("o", "x"));
    let t = format!("{b}\n{}", row("t", "y"));
    let merged = clean(&b, &o, &t);
    assert!(
        merged.starts_with(&format!("# file header\n\n{odd}\n")),
        "{merged}"
    );
    assert!(
        merged.contains("# about o\n[[probe]]\nid = \"o\""),
        "{merged}"
    );
}

#[test]
fn a_comment_above_a_row_travels_with_it() {
    let b = base();
    let o = format!("{b}\n# the story of ours\n{}", row("ours", "x"));
    let t = file(&[row("a", "1"), row("b", "2"), row("t", "y"), row("c", "3")]);
    let merged = clean(&b, &o, &t);
    assert!(
        merged.contains("# the story of ours\n[[probe]]\nid = \"ours\""),
        "{merged}"
    );
    assert_eq!(ids(&merged), ["a", "b", "t", "c", "ours"]);
}

#[test]
fn rows_added_to_a_file_with_no_blank_lines_stay_readable() {
    let b = "[[probe]]\nid = \"a\"\n[[probe]]\nid = \"b\"\n";
    let o = format!("{b}[[probe]]\nid = \"o\"\n");
    let t = format!("{b}[[probe]]\nid = \"t\"\n");
    let merged = clean(b, &o, &t);
    assert_eq!(
        merged,
        format!("{b}[[probe]]\nid = \"o\"\n[[probe]]\nid = \"t\"\n")
    );
}

#[test]
fn rows_added_from_empty_by_both_sides_are_both_kept() {
    let merged = clean("", &row("o", "x"), &row("t", "y"));
    assert_eq!(ids(&merged), ["o", "t"]);
}

#[test]
fn a_row_one_side_moved_stays_moved() {
    let b = base();
    let moved = file(&[row("c", "3"), row("a", "1"), row("b", "2")]);
    let added = file(&[row("a", "1"), row("b", "2"), row("t", "y"), row("c", "3")]);
    let merged = clean(&b, &moved, &added);
    assert_eq!(ids(&merged), ["c", "a", "b", "t"]);
    let merged = clean(&b, &added, &moved);
    assert_eq!(ids(&merged), ["c", "a", "b", "t"]);
}

#[test]
fn the_head_merges_like_a_row() {
    let b = format!("version = 1\n\n{}", base());
    let o = b.replace("version = 1", "version = 2");
    assert_eq!(clean(&b, &o, &b), o);
    assert_eq!(clean(&b, &b, &o), o);
    let t = b.replace("version = 1", "version = 3");
    let merged = conflicted(&b, &o, &t);
    assert!(
        merged.starts_with("<<<<<<< ours\nversion = 2\n=======\nversion = 3\n>>>>>>> theirs\n"),
        "{merged}"
    );
}

#[test]
fn a_plain_table_merges_like_a_row() {
    let b = format!("{}\n[meta]\nk = 1\n", base());
    let o = b.replace("k = 1", "k = 2");
    assert_eq!(clean(&b, &o, &b), o);
    let t = b.replace("k = 1", "k = 3");
    conflicted(&b, &o, &t);
}

#[test]
fn a_row_in_a_second_namespace_is_keyed_apart_from_the_same_id_in_the_first() {
    let other = |id: &str| format!("[[law]]\nid = \"{id}\"\n");
    let b = base();
    let o = format!("{b}\n{}", other("a"));
    let t = format!("{b}\n{}", row("a2", ""));
    let merged = clean(&b, &o, &t);
    assert_eq!(ids(&merged).len(), 5, "{merged}");
}

#[test]
fn a_file_it_cannot_read_as_rows_is_handed_back() {
    let b = base();
    let broken = "[[probe]\nid = \"a\"\n";
    assert!(merge_text(&b, broken, &b, 7).is_err());
    let dup = format!("{b}\n{}", row("a", "again"));
    assert!(merge_text(&b, &dup, &b, 7).is_err());
}

#[test]
fn a_subtable_belongs_to_its_row_and_moves_with_it() {
    let sub = |t: &str| format!("[[probe]]\nid = \"a\"\n\n[probe.meta]\nk = \"{t}\"\n");
    let b = format!("{}\n{}", sub("1"), row("b", "2"));
    let o = format!("{}\n{}", sub("ours"), row("b", "2"));
    assert_eq!(clean(&b, &o, &b), o);
    let t = format!("{}\n{}\n{}", sub("1"), row("b", "2"), row("t", "y"));
    let merged = clean(&b, &o, &t);
    assert!(
        merged.contains("k = \"ours\"") && merged.contains("id = \"t\""),
        "{merged}"
    );
}
