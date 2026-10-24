//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// ── decide: the whole matrix, asked x named x record ──

#[test]
fn decide_answers_every_one_of_the_twelve_cases() {
    use Run::*;
    use Skip::*;
    use Verdict::{Run as R, Skip as S};
    let cases: [(bool, bool, Option<&str>, Verdict); 12] = [
        (false, false, None, R(NeverGreen)),
        (false, false, Some("x"), S(Unchanged)),
        (false, false, Some("y"), R(Moved)),
        (false, true, None, S(OtherCratesNamed)),
        (false, true, Some("x"), S(OtherCratesNamed)),
        (false, true, Some("y"), S(OtherCratesNamed)),
        (true, false, None, R(Asked)),
        (true, false, Some("x"), R(Asked)),
        (true, false, Some("y"), R(Asked)),
        (true, true, None, R(Asked)),
        (true, true, Some("x"), R(Asked)),
        (true, true, Some("y"), R(Asked)),
    ];
    for (asked, named, rec, want) in cases {
        assert_eq!(
            decide(asked, named, "x", rec),
            want,
            "asked={asked} named={named} recorded={rec:?}"
        );
    }
}

// ── the record ──

#[test]
fn a_green_record_reads_back_and_a_missing_one_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(recorded(tmp.path()), None);
    record_green(tmp.path(), "abc");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("abc"));
    record_green(tmp.path(), "def");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("def"));
}

/// An edit made while the arms were building must not be recorded as passed.
#[test]
fn only_a_tree_that_did_not_move_during_the_run_is_recorded() {
    assert!(recordable(Some("a"), Some("a")));
    assert!(!recordable(Some("a"), Some("b")));
    assert!(!recordable(None, Some("a")));
    assert!(!recordable(Some("a"), None));
}

// ── the last line of the run ──

#[test]
fn the_summary_names_a_skipped_benches_tree_and_why() {
    let s = summary(0, Some(Skip::Unchanged));
    assert!(s.contains("benches not run"), "{s}");
    assert!(s.contains(Skip::Unchanged.describe()), "{s}");
    let s = summary(2, Some(Skip::OtherCratesNamed));
    assert!(s.contains("2 tree(s) green") && s.contains("-p"), "{s}");
    assert_eq!(summary(3, None), "mock test: 3 tree(s) green");
}

// ── what reads the digest and the record ──

#[test]
fn the_digest_and_the_record_are_read_only_where_the_verdict_needs_them() {
    use std::cell::Cell;
    let reads = |asked, named, record: Option<&str>| {
        let (d, r) = (Cell::new(0), Cell::new(0));
        let v = verdict(
            asked,
            named,
            || {
                d.set(d.get() + 1);
                Some("x".to_string())
            },
            || {
                r.set(r.get() + 1);
                record.map(str::to_string)
            },
        );
        (v, d.get(), r.get())
    };
    // Asked, or named crates: the answer does not depend on either.
    assert_eq!(
        reads(true, false, Some("x")),
        (Verdict::Run(Run::Asked), 0, 0)
    );
    assert_eq!(reads(true, true, None), (Verdict::Run(Run::Asked), 0, 0));
    assert_eq!(
        reads(false, true, Some("y")),
        (Verdict::Skip(Skip::OtherCratesNamed), 0, 0)
    );
    // Otherwise each is read once.
    assert_eq!(
        reads(false, false, Some("x")),
        (Verdict::Skip(Skip::Unchanged), 1, 1)
    );
    assert_eq!(
        reads(false, false, Some("y")),
        (Verdict::Run(Run::Moved), 1, 1)
    );
    assert_eq!(
        reads(false, false, None),
        (Verdict::Run(Run::NeverGreen), 1, 1)
    );
}

#[test]
fn a_tree_whose_inputs_cannot_be_listed_runs_whatever_was_recorded() {
    let v = verdict(false, false, || None, || Some("".to_string()));
    assert_eq!(v, Verdict::Run(Run::NeverGreen));
    let v = verdict(false, false, || None, || Some("x".to_string()));
    assert_eq!(v, Verdict::Run(Run::NeverGreen));
}

// ── the last lines of a failed run ──

#[test]
fn a_failed_run_names_a_skipped_benches_tree_too() {
    let s = failure_summary(1, 3, Some(Skip::OtherCratesNamed));
    assert!(s.contains("1 of 3 tree(s) failed"), "{s}");
    assert!(s.contains("benches not run") && s.contains("-p"), "{s}");
    assert_eq!(
        failure_summary(1, 3, None),
        "mock test: 1 of 3 tree(s) failed"
    );
}
