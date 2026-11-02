//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A binary with a red test settles the tests that passed beside it.

use super::*;
use crate::suite::history::Red;

fn case(bin: &str, name: &str, outcome: Outcome) -> Case {
    Case {
        binary: bin.to_string(),
        name: name.to_string(),
        secs: 0.1,
        outcome,
    }
}

/// `render::b` ran whole at its fingerprint and failed `two`; `one` and `three`
/// passed beside it.
fn run_with_one_red() -> (Fingerprints, History) {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let cases = vec![
        case("render::b", "one", Outcome::Pass),
        case("render::b", "two", Outcome::Fail),
        case("render::b", "three", Outcome::Pass),
        case("render::a", "x", Outcome::Pass),
    ];
    let o = binary_outcomes(&p, &cases, &fp, true);
    assert_eq!(o.failed, vec!["render::b"]);
    assert_eq!(o.red, vec![("render::b".to_string(), Red {
        at:    fp.bins["render::b"].clone(),
        names: vec!["two".to_string()],
    })]);
    let mut h = History::default();
    o.apply(h.flavour_mut(""));
    (fp, h)
}

#[test]
fn the_next_run_at_the_same_inputs_runs_the_failed_test_alone() {
    let (fp, h) = run_with_one_red();
    // The member is not green, so it runs; `a` passed and is left out, and `b`
    // is narrowed to `two` rather than run whole.
    let p = plan_for(Mode::Changed, true, &fp, &h);
    assert!(names(&p).contains(&"render"));
    assert_eq!(p.reuse, vec!["render::a"]);
    assert_eq!(
        p.rerun,
        BTreeMap::from([("render::b".to_string(), vec!["two".to_string()])])
    );
}

/// The case that must fail: the binary moved, so the earlier run says nothing
/// of it and it runs whole.
#[test]
fn a_binary_that_moved_since_it_failed_runs_whole() {
    let (_, h) = run_with_one_red();
    let moved = with_bins(fps(&[]), &[("render::b", 1)]);
    let p = plan_for(Mode::Changed, true, &moved, &h);
    assert!(p.rerun.is_empty());
}

#[test]
fn a_full_run_or_no_cache_takes_nothing_on_the_earlier_failure() {
    let (fp, h) = run_with_one_red();
    assert!(plan_for(Mode::Full, true, &fp, &h).rerun.is_empty());
    assert!(plan_for(Mode::Changed, false, &fp, &h).rerun.is_empty());
}

/// A rerun that passes settles the binary; one that fails again keeps the red
/// set to what failed now; a test gone from the binary runs it whole.
#[test]
fn a_rerun_settles_the_binary_or_names_what_still_fails() {
    let (fp, mut h) = run_with_one_red();
    let p = plan_for(Mode::Changed, true, &fp, &h);
    let pass = binary_outcomes(&p, &[case("render::b", "two", Outcome::Pass)], &fp, true);
    pass.apply(h.flavour_mut(""));
    let f = h.flavour("").unwrap();
    assert_eq!(f.bins["render::b"], fp.bins["render::b"]);
    assert!(f.red.is_empty());

    let (fp, mut h) = run_with_one_red();
    let p = plan_for(Mode::Changed, true, &fp, &h);
    let again = binary_outcomes(&p, &[case("render::b", "two", Outcome::Fail)], &fp, true);
    again.apply(h.flavour_mut(""));
    let f = h.flavour("").unwrap();
    assert!(!f.bins.contains_key("render::b"));
    assert_eq!(f.red["render::b"].names, vec!["two"]);

    let (fp, h) = run_with_one_red();
    let mut p = plan_for(Mode::Changed, true, &fp, &h);
    p.retain_live(&live(&[
        ("render::a", &["x"]),
        ("render::b", &["one", "three"]),
    ]));
    assert!(
        p.rerun.is_empty(),
        "`two` is gone, so nothing sound to narrow to"
    );
}

/// A binary with a heavy test the run deferred was not run whole, so it is no
/// base for narrowing.
#[test]
fn a_failed_binary_with_a_deferred_test_is_not_narrowed_to_its_failures() {
    let fp = with_bins(fps(&[]), &[]);
    let mut p = plan_for(Mode::Full, false, &fp, &History::default());
    p.skip.push(Skip {
        binary: "render::b".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Deferred,
    });
    let o = binary_outcomes(&p, &[case("render::b", "two", Outcome::Fail)], &fp, true);
    assert_eq!(o.failed, vec!["render::b"]);
    assert!(o.red.is_empty());
}

#[test]
fn an_incomplete_run_records_no_red_set() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let o = binary_outcomes(&p, &[case("render::b", "two", Outcome::Fail)], &fp, false);
    assert!(o.red.is_empty() && o.failed.is_empty());
}

#[test]
fn a_filter_that_ran_nothing_while_the_plan_left_tests_out_is_refused() {
    let (fp, h) = run_with_one_red();
    let p = plan_for(Mode::Changed, true, &fp, &h);
    let refusal = empty_filtered_run(&p, &[], true).expect("a refusal");
    assert!(
        refusal.contains("1 binaries passed") && refusal.contains("1 narrowed"),
        "{refusal}"
    );
    // Skipped cases ran nothing either.
    assert!(empty_filtered_run(&p, &[case("render::a", "x", Outcome::Skipped)], true).is_some());
    // The controls: a test ran, or nobody filtered, or nothing was left out.
    assert!(empty_filtered_run(&p, &[case("render::b", "two", Outcome::Pass)], true).is_none());
    assert!(empty_filtered_run(&p, &[], false).is_none());
    assert!(empty_filtered_run(&Plan::default(), &[], true).is_none());
}
