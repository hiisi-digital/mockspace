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
    let o = binary_outcomes(&p, &cases, &fp, true, Some(&listed()));
    assert_eq!(o.failed, vec!["render::b"]);
    assert_eq!(o.red, vec![("render::b".to_string(), Red {
        at:     fp.bins["render::b"].clone(),
        names:  vec!["two".to_string()],
        listed: crate::suite::live::digest(&listed()["render::b"]),
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
    let pass = binary_outcomes(
        &p,
        &[case("render::b", "two", Outcome::Pass)],
        &fp,
        true,
        Some(&listed()),
    );
    pass.apply(h.flavour_mut(""));
    let f = h.flavour("").unwrap();
    assert_eq!(f.bins["render::b"], fp.bins["render::b"]);
    assert!(f.red.is_empty());

    let (fp, mut h) = run_with_one_red();
    let p = plan_for(Mode::Changed, true, &fp, &h);
    let again = binary_outcomes(
        &p,
        &[case("render::b", "two", Outcome::Fail)],
        &fp,
        true,
        Some(&listed()),
    );
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
    // Every listed test is in the report or skipped, so the binary is whole and
    // only the deferred test stops it being narrowed to.
    let mut listing = listed();
    listing
        .get_mut("render::b")
        .unwrap()
        .insert("drawn".to_string());
    let cases = [
        case("render::b", "one", Outcome::Pass),
        case("render::b", "two", Outcome::Fail),
        case("render::b", "three", Outcome::Pass),
    ];
    let o = binary_outcomes(&p, &cases, &fp, true, Some(&listing));
    assert_eq!(o.failed, vec!["render::b"]);
    assert!(o.red.is_empty());
    // The control: the same report with that test skipped as cached is whole.
    p.skip[0].why = SkipWhy::Cached;
    assert_eq!(
        binary_outcomes(&p, &cases, &fp, true, Some(&listing))
            .red
            .len(),
        1
    );
}

#[test]
fn an_incomplete_run_records_no_red_set() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let o = binary_outcomes(
        &p,
        &[case("render::b", "two", Outcome::Fail)],
        &fp,
        false,
        Some(&listed()),
    );
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

fn listed() -> crate::suite::live::Live {
    live(&[("render::a", &["x"]), ("render::b", &["one", "three", "two"])])
}

/// `--fail-fast` stops after the first failure and nextest still exits as for a
/// finished run, with the tests it never started missing from the report. Those
/// are not passed, so nothing may be narrowed to the failure and a rerun that
/// passes must not make the binary pass.
#[test]
fn a_run_cut_short_by_its_first_failure_is_not_a_whole_run() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let cut = [case("render::b", "two", Outcome::Fail)];
    let o = binary_outcomes(&p, &cut, &fp, true, Some(&listed()));
    assert_eq!(o.failed, vec!["render::b"]);
    assert!(o.red.is_empty(), "{:?}", o.red);
    // The control: the same failure in a report that holds every listed test.
    let whole = [
        case("render::b", "one", Outcome::Pass),
        case("render::b", "two", Outcome::Fail),
        case("render::b", "three", Outcome::Pass),
    ];
    assert_eq!(
        binary_outcomes(&p, &whole, &fp, true, Some(&listed()))
            .red
            .len(),
        1
    );
    // And a pass that left two listed tests out of its report is no pass.
    let some = [case("render::b", "one", Outcome::Pass)];
    assert!(
        binary_outcomes(&p, &some, &fp, true, Some(&listed()))
            .passed
            .is_empty()
    );
    // Where the listing is unknown nothing is narrowed to, and a pass is as before.
    assert!(binary_outcomes(&p, &cut, &fp, true, None).red.is_empty());
    assert_eq!(binary_outcomes(&p, &some, &fp, true, None).passed.len(), 1);
}

fn listing_with(extra: &[(&str, &[&str])]) -> crate::suite::live::Live {
    let mut l = listed();
    for (bin, tests) in extra {
        l.insert(
            bin.to_string(),
            tests.iter().map(|t| t.to_string()).collect(),
        );
    }
    l
}

/// A member whose binaries never started because a run stopped at its first
/// failure is not green, though nextest exited as for a finished run.
#[test]
fn a_member_whose_binary_was_never_started_is_not_green() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let listing = listing_with(&[("mesh", &["m"]), ("web", &["w"])]);
    let cut = [case("render::b", "two", Outcome::Fail)];
    // `mesh` and `web` each list a test the report never reached.
    let green = greens(&p, &cut, true, Some(&listing));
    assert!(
        !green.contains(&"mesh".to_string()) && !green.contains(&"web".to_string()),
        "{green:?}"
    );
    assert!(!green.contains(&"render".to_string()));
    // The control: with their tests in the report they are green.
    let all = [
        case("render::b", "two", Outcome::Fail),
        case("mesh", "m", Outcome::Pass),
        case("web", "w", Outcome::Pass),
    ];
    let green = greens(&p, &all, true, Some(&listing));
    assert!(
        green.contains(&"mesh".to_string()) && green.contains(&"web".to_string()),
        "{green:?}"
    );
    // And with no listing, a failure leaves nothing settled.
    assert!(greens(&p, &cut, true, None).is_empty());
    assert!(!greens(&p, &[case("mesh", "m", Outcome::Pass)], true, None).is_empty());
}

#[test]
fn with_no_listing_a_failure_beside_a_partial_pass_settles_no_pass() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let cases = [case("render::a", "x", Outcome::Pass), case("render::b", "two", Outcome::Fail)];
    let o = binary_outcomes(&p, &cases, &fp, true, None);
    assert!(o.passed.is_empty(), "{:?}", o.passed);
    assert_eq!(o.failed, vec!["render::b"]);
    // The control: without a failure a pass is judged as before.
    let ok = [case("render::a", "x", Outcome::Pass)];
    assert_eq!(binary_outcomes(&p, &ok, &fp, true, None).passed.len(), 1);
}

/// A test that appears in the binary at an unchanged fingerprint has never run,
/// so a rerun of the failures says nothing of it.
#[test]
fn a_binary_that_lists_a_test_it_did_not_when_it_failed_runs_whole() {
    let (fp, h) = run_with_one_red();
    let mut p = plan_for(Mode::Changed, true, &fp, &h);
    assert!(!p.rerun.is_empty());
    p.retain_live(&listing_with(&[("render::b", &[
        "one", "two", "three", "four",
    ])]));
    assert!(p.rerun.is_empty() && p.rerun_listed.is_empty());
    // The control: the same listing as when it failed keeps the narrowing.
    let mut same = plan_for(Mode::Changed, true, &fp, &h);
    same.retain_live(&listed());
    assert_eq!(same.rerun.len(), 1);
}

/// A binary that fails again in a run that did not cover it whole loses the red
/// set it had; a pass of the rest cannot then settle it.
#[test]
fn a_failure_replaces_the_red_set_it_found() {
    let (fp, mut h) = run_with_one_red();
    let p = plan_for(Mode::Full, false, &fp, &h);
    let cut = [case("render::b", "one", Outcome::Fail)];
    let o = binary_outcomes(&p, &cut, &fp, true, Some(&listed()));
    assert_eq!(o.failed, vec!["render::b"]);
    assert!(o.red.is_empty());
    o.apply(h.flavour_mut(""));
    assert!(
        h.flavour("").unwrap().red.is_empty(),
        "the old narrowing to `two` must go"
    );
}

/// What the plan took as already passed is covered by it.
#[test]
fn a_test_skipped_as_passed_counts_as_covered() {
    let fp = with_bins(fps(&[]), &[]);
    let mut p = plan_for(Mode::Full, false, &fp, &History::default());
    p.skip.push(Skip {
        binary: "render::b".to_string(),
        name:   "three".to_string(),
        why:    SkipWhy::Cached,
    });
    let cases = [case("render::b", "one", Outcome::Pass), case("render::b", "two", Outcome::Pass)];
    assert_eq!(
        binary_outcomes(&p, &cases, &fp, true, Some(&listed()))
            .passed
            .len(),
        1
    );
    p.skip.clear();
    assert!(
        binary_outcomes(&p, &cases, &fp, true, Some(&listed()))
            .passed
            .is_empty()
    );
}

/// A binary is narrowed to its failures only when no whole pass of it stands; one
/// that did pass is left out whole and is not also narrowed.
#[test]
fn a_binary_left_out_whole_is_not_also_narrowed() {
    let (fp, mut h) = run_with_one_red();
    h.flavour_mut("")
        .bins
        .insert("render::b".to_string(), fp.bins["render::b"].clone());
    let p = plan_for(Mode::Changed, true, &fp, &h);
    assert!(p.reuse.contains(&"render::b".to_string()));
    assert!(p.rerun.is_empty());
}
