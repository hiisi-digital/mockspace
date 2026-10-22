//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;
use crate::changelist_helpers::{ClKind, ClStatus, parse_changelist};

// Each test names the mutant that turns it red.

#[test]
fn an_old_bare_stamp_is_still_an_id() {
    // Mutant: `split_prefix` demands a tag.
    let (id, rest) = split_prefix("202603071430_changelist.doc.md").unwrap();
    assert_eq!((id, rest), ("202603071430", "_changelist.doc.md"));
    assert_eq!(base_id("202603071430"), Some("202603071430"));
}

#[test]
fn a_tagged_id_is_read_whole() {
    // Mutant: `split_prefix` reads twelve characters and stops.
    let (id, rest) = split_prefix("202610221529-kx3f_topic.a.md").unwrap();
    assert_eq!((id, rest), ("202610221529-kx3f", "_topic.a.md"));
}

#[test]
fn a_tagged_changelist_parses_like_an_old_one() {
    // Mutant: `strip_timestamp_prefix` goes back to twelve fixed digits.
    let cl = parse_changelist("202610221529-kx3f_changelist.src.lock.md").unwrap();
    assert_eq!((cl.kind, cl.status), (ClKind::Src, ClStatus::Locked));
    assert!(parse_changelist("202610221529_changelist.src.lock.md").is_some());
}

#[test]
fn abandoned_is_not_a_tag() {
    // `aban` is four tag characters. Mutant: the `closes` test in `split_prefix`.
    assert_eq!(base_id("202610221529-abandoned"), Some("202610221529"));
    assert_eq!(kept_tail("202610221529-abandoned"), "-abandoned");
    assert_eq!(kept_tail("202610221529-kx3f-abandoned"), "-abandoned");
    assert_eq!(kept_tail("202610221529-kx3f-2"), "");
}

#[test]
fn a_digit_first_or_wrong_length_tag_is_not_one() {
    // Mutant: `is_tag` drops the letter-first test, so `-2026` reads as a tag
    // and a `-N` collision suffix is taken for one.
    assert!(!is_tag("1abc"));
    assert!(!is_tag("abc"));
    assert!(!is_tag("abcde"));
    assert!(!is_tag("ABCD"));
    assert_eq!(base_id("202610221529-2026"), Some("202610221529"));
}

#[test]
fn round_dirs_are_ids_with_an_optional_count() {
    // Mutant: `is_round_dir` accepts any suffix, or none but digits.
    for yes in [
        "202610221529",
        "202610221529-2",
        "202610221529-kx3f",
        "202610221529-kx3f-2",
    ] {
        assert!(is_round_dir(yes), "{yes}");
    }
    for no in [
        "202610221529-abandoned",
        "202610221529-kx3f-abandoned",
        "202610221529-",
        "202610221529-kx3f-",
        "20261022152",
        "2026-10-22",
        "README.md",
    ] {
        assert!(!is_round_dir(no), "{no}");
    }
}

#[test]
fn ids_sort_in_time_order_across_old_and_new() {
    // Mutant: the stamp moves behind the tag, or the tag is put first.
    let mut names = vec![
        "202610221531_topic.z.md",
        "202610221529-zzzz_topic.b.md",
        "202610221530_topic.c.md",
        "202610221529_topic.a.md",
        "202610221528-aaaa_topic.0.md",
    ];
    names.sort();
    let ids: Vec<&str> = names.iter().map(|n| split_prefix(n).unwrap().0).collect();
    let stamps: Vec<&str> = ids.iter().map(|id| &id[.. 12]).collect();
    let mut sorted = stamps.clone();
    sorted.sort();
    assert_eq!(stamps, sorted, "{ids:?}");
    assert_eq!(ids[0], "202610221528-aaaa");
    assert_eq!(ids[4], "202610221531");
}

#[test]
fn fresh_ids_are_tagged_and_deterministic_for_a_seed() {
    // Mutant: `tag_for` ignores the seed.
    let a = fresh("202610221529", "branch-a", |_| false);
    let b = fresh("202610221529", "branch-b", |_| false);
    assert_eq!(a, fresh("202610221529", "branch-a", |_| false));
    assert_ne!(a, b, "two seeds in one minute must not share an id");
    assert!(split_prefix(&a).is_some_and(|(id, rest)| id == a && rest.is_empty()));
}

#[test]
fn a_taken_id_is_walked_past() {
    // Mutant: `fresh` returns the first candidate whatever `taken` says.
    let first = fresh("202610221529", "s", |_| false);
    let second = fresh("202610221529", "s", |c| c == first);
    assert_ne!(first, second);
    let third = fresh("202610221529", "s", |c| c == first || c == second);
    assert!(third != first && third != second);
}

#[test]
fn the_first_character_of_every_tag_is_a_letter() {
    // Mutant: `tag[0]` drawn from the digits as well.
    for n in 0 .. 5000 {
        let tag = tag_for(&format!("seed{n}"), n % 7);
        assert!(is_tag(&tag), "{tag}");
    }
}

#[test]
fn rename_prefix_moves_exactly_one_id() {
    // Mutant: `rename_prefix` is a `starts_with`, so the stamp of a tagged id is
    // taken for the untagged round and a longer id is cut in half.
    let new = "202610221529-kx3f";
    assert_eq!(
        rename_prefix("202610221529_changelist.doc.md", "202610221529", new).as_deref(),
        Some("202610221529-kx3f_changelist.doc.md")
    );
    assert_eq!(
        rename_prefix("202610221529-zz9z_changelist.doc.md", "202610221529", new),
        None
    );
    assert_eq!(rename_prefix("202610221529-zz9z.md", "202610221529-zz9z", new), None);
    assert_eq!(rename_prefix("README.md", "202610221529", new), None);
}
