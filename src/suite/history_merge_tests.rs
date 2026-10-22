//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::BTreeMap;

use super::*;
use crate::suite::history::Green;

fn green(tag: &str) -> Green {
    Green {
        full: tag.to_string(),
        own:  BTreeMap::from([("render".to_string(), tag.to_string())]),
    }
}

fn rec(secs: f64, passed: bool, at: Option<&str>) -> TestRecord {
    TestRecord {
        secs,
        passed,
        passed_at: at.map(str::to_string),
    }
}

fn file(green: Option<&str>) -> MemberFile {
    MemberFile {
        version: VERSION,
        flavour: "default".into(),
        member: "render".into(),
        green: green.map(self::green),
        ..MemberFile::default()
    }
}

fn with_test(mut f: MemberFile, bin: &str, name: &str, r: TestRecord) -> MemberFile {
    f.tests
        .entry(bin.into())
        .or_default()
        .insert(name.into(), r);
    f
}

fn with_bin(mut f: MemberFile, bin: &str, at: &str) -> MemberFile {
    f.bins.insert(bin.into(), at.into());
    f
}

fn merged(base: &MemberFile, ours: &MemberFile, theirs: &MemberFile, later: Later) -> MemberFile {
    let m = merge_text(&base.render(), &ours.render(), &theirs.render(), later).unwrap();
    MemberFile::parse(&m.text).unwrap()
}

#[test]
fn identical_sides_merge_to_themselves() {
    let f = with_test(
        file(Some("g1")),
        "render",
        "drawn",
        rec(40.0, true, Some("b1")),
    );
    let f = with_bin(f, "render::drawn", "b1");
    assert_eq!(merged(&f, &f, &f, Later::Unknown), f);
}

#[test]
fn one_side_unchanged_takes_the_other_whole() {
    let base = with_test(
        file(Some("g0")),
        "render",
        "drawn",
        rec(40.0, true, Some("b0")),
    );
    let moved = with_test(
        file(Some("g1")),
        "render",
        "drawn",
        rec(55.0, true, Some("b1")),
    );
    let moved = with_bin(moved, "render::drawn", "b1");
    for later in [Later::Ours, Later::Theirs, Later::Unknown] {
        assert_eq!(
            merged(&base, &base, &moved, later),
            moved,
            "ours unchanged, {later:?}"
        );
        assert_eq!(
            merged(&base, &moved, &base, later),
            moved,
            "theirs unchanged, {later:?}"
        );
    }
}

#[test]
fn both_changed_the_same_keeps_it() {
    let base = file(Some("g0"));
    let same = with_test(
        file(Some("g1")),
        "render",
        "drawn",
        rec(50.0, true, Some("b1")),
    );
    assert_eq!(merged(&base, &same, &same, Later::Unknown), same);
}

#[test]
fn both_changed_the_green_differently_drops_it_and_says_so() {
    let base = file(Some("g0"));
    let (ours, theirs) = (file(Some("g1")), file(Some("g2")));
    let m = merge_text(
        &base.render(),
        &ours.render(),
        &theirs.render(),
        Later::Ours,
    )
    .unwrap();
    assert!(m.dropped_green);
    assert_eq!(MemberFile::parse(&m.text).unwrap().green, None);
    // the other way round, and with the later side the other one
    let m = merge_text(
        &base.render(),
        &theirs.render(),
        &ours.render(),
        Later::Theirs,
    )
    .unwrap();
    assert_eq!(MemberFile::parse(&m.text).unwrap().green, None);
}

#[test]
fn a_green_one_side_nulled_and_the_other_left_stays_nulled() {
    // the base's green, one side lost it to a cut-short run, the other never touched it
    let base = file(Some("g0"));
    let nulled = file(None);
    assert_eq!(merged(&base, &nulled, &base, Later::Unknown).green, None);
    assert_eq!(merged(&base, &base, &nulled, Later::Unknown).green, None);
}

#[test]
fn a_green_added_on_both_sides_with_no_base_settles_like_any_other() {
    let (a, b) = (file(Some("g1")), file(Some("g2")));
    let m = merge_text("", &a.render(), &b.render(), Later::Unknown).unwrap();
    assert!(m.dropped_green);
    let m = merge_text("", &a.render(), &a.render(), Later::Unknown).unwrap();
    assert!(!m.dropped_green);
    assert_eq!(MemberFile::parse(&m.text).unwrap().green, Some(green("g1")));
}

#[test]
fn the_green_rule_over_every_triple_of_none_and_two_values() {
    let vals = [None, Some("a"), Some("b")];
    for b in vals {
        for o in vals {
            for t in vals {
                let (base, ours, theirs) = (file(b), file(o), file(t));
                let got = merged(&base, &ours, &theirs, Later::Unknown).green;
                let swapped = merged(&base, &theirs, &ours, Later::Unknown).green;
                let case = format!("base {b:?} ours {o:?} theirs {t:?}");
                // never a value neither side holds
                assert!(
                    got.is_none() || got == ours.green || got == theirs.green,
                    "invented a green: {case}"
                );
                // the order of the sides does not matter
                assert_eq!(got, swapped, "asymmetric: {case}");
                if o == t {
                    assert_eq!(got, ours.green, "agreed greens stand: {case}");
                } else if o == b {
                    assert_eq!(got, theirs.green, "ours unmoved takes theirs: {case}");
                } else if t == b {
                    assert_eq!(got, ours.green, "theirs unmoved takes ours: {case}");
                } else {
                    // both moved, to different places: no side is ever chosen
                    assert_eq!(got, None, "a side was selected for a green: {case}");
                }
            }
        }
    }
}

#[test]
fn a_test_on_one_side_only_is_kept_and_so_is_each_sides_own() {
    let base = file(None);
    let ours = with_test(
        file(None),
        "render",
        "from_ours",
        rec(30.0, true, Some("b")),
    );
    let theirs = with_test(file(None), "render", "from_theirs", rec(20.0, false, None));
    let got = merged(&base, &ours, &theirs, Later::Unknown);
    assert_eq!(got.tests["render"].keys().collect::<Vec<_>>(), [
        "from_ours",
        "from_theirs"
    ]);
    assert_eq!(got.tests["render"]["from_ours"], rec(30.0, true, Some("b")));
    assert_eq!(got.tests["render"]["from_theirs"], rec(20.0, false, None));
}

#[test]
fn a_test_the_base_had_and_one_side_removed_stays_removed() {
    let base = with_test(file(None), "render", "gone", rec(30.0, true, Some("b")));
    let removed = file(None);
    assert!(
        merged(&base, &removed, &base, Later::Unknown)
            .tests
            .is_empty()
    );
    assert!(
        merged(&base, &base, &removed, Later::Unknown)
            .tests
            .is_empty()
    );
    // removed on one side and retimed on the other: the retimed one is kept
    let retimed = with_test(file(None), "render", "gone", rec(90.0, true, Some("c")));
    assert_eq!(
        merged(&base, &removed, &retimed, Later::Unknown).tests["render"]["gone"],
        rec(90.0, true, Some("c"))
    );
}

#[test]
fn a_test_timed_on_both_sides_takes_the_later_measurement() {
    let base = with_test(file(None), "render", "t", rec(30.0, true, Some("b0")));
    let ours = with_test(file(None), "render", "t", rec(45.0, true, Some("b1")));
    let theirs = with_test(file(None), "render", "t", rec(20.0, true, Some("b2")));
    let at = |later| merged(&base, &ours, &theirs, later).tests["render"]["t"].clone();
    assert_eq!(at(Later::Ours), rec(45.0, true, Some("b1")));
    assert_eq!(at(Later::Theirs), rec(20.0, true, Some("b2")));
    // no clock: the longer timing
    assert_eq!(at(Later::Unknown), rec(45.0, true, Some("b1")));
}

#[test]
fn with_no_clock_a_tie_goes_to_the_failure_and_the_order_never_matters() {
    let base = with_test(file(None), "render", "t", rec(30.0, true, Some("b0")));
    let pass = with_test(file(None), "render", "t", rec(40.0, true, Some("b1")));
    let fail = with_test(file(None), "render", "t", rec(40.0, false, None));
    let other = with_test(file(None), "render", "t", rec(40.0, true, Some("b2")));
    for (x, y) in [(&pass, &fail), (&pass, &other)] {
        let one = merged(&base, x, y, Later::Unknown);
        let two = merged(&base, y, x, Later::Unknown);
        assert_eq!(
            one, two,
            "a merge with no clock is the same either way round"
        );
    }
    assert!(!merged(&base, &pass, &fail, Later::Unknown).tests["render"]["t"].passed);
}

#[test]
fn a_test_only_one_side_moved_takes_that_move_whatever_the_clock_says() {
    let base = with_test(file(None), "render", "t", rec(30.0, true, Some("b0")));
    let moved = with_test(file(None), "render", "t", rec(45.0, true, Some("b1")));
    for later in [Later::Ours, Later::Theirs, Later::Unknown] {
        assert_eq!(
            merged(&base, &base, &moved, later).tests["render"]["t"],
            rec(45.0, true, Some("b1"))
        );
        assert_eq!(
            merged(&base, &moved, &base, later).tests["render"]["t"],
            rec(45.0, true, Some("b1"))
        );
    }
}

#[test]
fn a_binarys_recorded_pass_settles_like_a_green() {
    let base = with_bin(file(None), "render::a", "x0");
    let ours = with_bin(with_bin(file(None), "render::a", "x1"), "render::b", "y1");
    let theirs = with_bin(with_bin(file(None), "render::a", "x2"), "render::c", "z1");
    let m = merge_text(
        &base.render(),
        &ours.render(),
        &theirs.render(),
        Later::Ours,
    )
    .unwrap();
    let got = MemberFile::parse(&m.text).unwrap();
    // `a` moved both ways and is dropped, `b` and `c` were each added on one side
    assert_eq!(got.bins.keys().collect::<Vec<_>>(), [
        "render::b",
        "render::c"
    ]);
    assert_eq!(m.dropped_bins, 1);
    assert!(!m.dropped_green);
}

#[test]
fn the_merged_text_is_what_a_run_would_have_written() {
    let f = with_test(
        file(Some("g1")),
        "render",
        "drawn",
        rec(40.0, true, Some("b1")),
    );
    let m = merge_text(&f.render(), &f.render(), &f.render(), Later::Unknown).unwrap();
    assert_eq!(m.text, f.render());
    assert!(m.text.ends_with("}\n"));
}

#[test]
fn a_version_one_file_meeting_a_version_two_one_is_refused() {
    let v2 = file(Some("g"));
    let v1 = MemberFile {
        version: PREVIOUS,
        ..file(Some("g"))
    };
    for (o, t) in [(&v1, &v2), (&v2, &v1)] {
        let err = merge_text(&v2.render(), &o.render(), &t.render(), Later::Unknown).unwrap_err();
        assert!(
            err.contains("version 1") && err.contains("version 2"),
            "{err}"
        );
    }
}

#[test]
fn two_version_one_files_merge_and_stay_version_one() {
    let v1 = |g: Option<&str>| {
        MemberFile {
            version: PREVIOUS,
            ..file(g)
        }
    };
    let m = merge_text(
        &v1(Some("g")).render(),
        &v1(Some("g")).render(),
        &v1(Some("g")).render(),
        Later::Unknown,
    )
    .unwrap();
    assert_eq!(MemberFile::parse(&m.text).unwrap().version, PREVIOUS);
}

#[test]
fn a_version_this_build_does_not_read_is_refused() {
    let future = MemberFile {
        version: 3,
        ..file(None)
    };
    let err = merge_text("", &future.render(), &future.render(), Later::Unknown).unwrap_err();
    assert!(err.contains("version 3"), "{err}");
}

#[test]
fn files_of_different_members_or_flavours_are_refused() {
    let a = file(None);
    let other_member = MemberFile {
        member: "audio".into(),
        ..file(None)
    };
    let other_flavour = MemberFile {
        flavour: "editor".into(),
        ..file(None)
    };
    for other in [&other_member, &other_flavour] {
        let err =
            merge_text(&a.render(), &a.render(), &other.render(), Later::Unknown).unwrap_err();
        assert!(
            err.contains("one file is one member in one flavour"),
            "{err}"
        );
    }
}

#[test]
fn a_file_that_is_not_a_record_is_refused_whichever_side_it_is() {
    let ok = file(Some("g")).render();
    for (o, t, which) in [
        ("{not json", ok.as_str(), "our"),
        (ok.as_str(), "", "their"),
        (ok.as_str(), "[]", "their"),
    ] {
        let err = merge_text(&ok, o, t, Later::Unknown).unwrap_err();
        assert!(
            err.contains(which) && err.contains("not a test-history record"),
            "{err}"
        );
    }
}

#[test]
fn a_base_that_is_not_a_record_of_the_same_kind_is_ignored() {
    // a base of another version is made of other fingerprints; as if there were none
    let (a, b) = (file(Some("g1")), file(Some("g2")));
    let v1_base = MemberFile {
        version: PREVIOUS,
        ..file(Some("g1"))
    };
    for base in [String::new(), "garbage".to_string(), v1_base.render()] {
        let m = merge_text(&base, &a.render(), &b.render(), Later::Unknown).unwrap();
        assert!(m.dropped_green, "base {base:?}");
    }
}

#[test]
fn a_file_with_no_bins_field_still_reads() {
    // a file written before `bins` existed has no such key
    let text = "{\n  \"version\": 2,\n  \"flavour\": \"default\",\n  \"member\": \"render\",\n  \"green\": null,\n  \"tests\": {}\n}\n";
    let m = merge_text(text, text, text, Later::Unknown).unwrap();
    assert_eq!(MemberFile::parse(&m.text).unwrap(), file(None));
}
