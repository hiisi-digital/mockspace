//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn case(binary: &str, outcome: Outcome) -> Case {
    Case {
        binary: binary.to_string(),
        name: "t".to_string(),
        secs: 0.0,
        outcome,
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn set(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_member_with_a_failed_test_is_recorded_and_one_that_passed_is_not() {
    let mut f = Failed::default();
    let cases = [case("a", Outcome::Fail), case("a", Outcome::Pass), case("b", Outcome::Pass)];
    f.settle("", &names(&["a", "b"]), &cases, true);
    assert_eq!(f.of(""), set(&["a"]));
}

/// A failure in an integration test binary is its package's.
#[test]
fn a_failed_binary_is_its_packages() {
    let mut f = Failed::default();
    f.settle("", &names(&["a"]), &[case("a::it", Outcome::Fail)], true);
    assert_eq!(f.of(""), set(&["a"]));
}

#[test]
fn a_member_that_passes_whole_is_cleared() {
    let mut f = Failed::default();
    f.settle("", &names(&["a"]), &[case("a", Outcome::Fail)], true);
    f.settle("", &names(&["a"]), &[case("a", Outcome::Pass)], true);
    assert!(f.of("").is_empty());
}

/// Run narrowed to one test, or cut short, a pass says nothing about the test
/// that failed: it stays recorded, and a new failure is added.
#[test]
fn a_narrowed_or_cut_short_run_clears_nothing_and_still_records() {
    let mut f = Failed::default();
    f.settle("", &names(&["a", "b"]), &[case("a", Outcome::Fail)], true);
    f.settle(
        "",
        &names(&["a", "b"]),
        &[case("a", Outcome::Pass), case("b", Outcome::Fail)],
        false,
    );
    assert_eq!(f.of(""), set(&["a", "b"]));
}

/// A member nothing ran for, because its tests were cached or its binaries
/// left out, has said nothing.
#[test]
fn a_member_that_ran_no_test_keeps_what_it_had() {
    let mut f = Failed::default();
    f.settle("", &names(&["a", "b"]), &[case("a", Outcome::Fail)], true);
    f.settle("", &names(&["a", "b"]), &[case("b", Outcome::Pass)], true);
    assert_eq!(f.of(""), set(&["a"]));
    // A member that was not run at all is not cleared either, whatever ran.
    f.settle(
        "",
        &names(&["b"]),
        &[case("a", Outcome::Pass), case("b", Outcome::Pass)],
        true,
    );
    assert_eq!(f.of(""), set(&["a"]));
}

#[test]
fn a_skipped_test_is_no_failure() {
    let mut f = Failed::default();
    f.settle("", &names(&["a"]), &[case("a", Outcome::Skipped)], true);
    assert!(f.of("").is_empty());
}

#[test]
fn each_flavour_keeps_its_own() {
    let mut f = Failed::default();
    f.settle(
        "--features x",
        &names(&["a"]),
        &[case("a", Outcome::Fail)],
        true,
    );
    assert_eq!(f.of("--features x"), set(&["a"]));
    assert!(f.of("").is_empty());
    f.settle("", &names(&["a"]), &[case("a", Outcome::Pass)], true);
    assert_eq!(
        f.of("--features x"),
        set(&["a"]),
        "another flavour's pass clears nothing here"
    );
}

#[test]
fn the_record_survives_the_file_and_a_bad_file_is_no_record() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(Failed::load(dir.path()), Failed::default());
    let mut f = Failed::default();
    f.settle("", &names(&["a"]), &[case("a", Outcome::Fail)], true);
    f.save(dir.path()).unwrap();
    assert_eq!(Failed::load(dir.path()), f);
    std::fs::write(Failed::path(dir.path()), "{ not json").unwrap();
    assert_eq!(Failed::load(dir.path()), Failed::default());
    // The control: the first load above was a real read of a missing file, and
    // this one a real read of a bad one; the good one in between is not empty.
    assert!(!f.of("").is_empty());
}

/// An empty flavour is no entry, so a settled suite leaves a record that says
/// nothing rather than one naming every flavour that ever ran.
#[test]
fn a_flavour_with_nothing_failed_leaves_no_entry() {
    let mut f = Failed::default();
    f.settle(
        "--features x",
        &names(&["a"]),
        &[case("a", Outcome::Fail)],
        true,
    );
    f.settle(
        "--features x",
        &names(&["a"]),
        &[case("a", Outcome::Pass)],
        true,
    );
    assert_eq!(f, Failed::default());
}

#[test]
fn a_plain_pass_takes_the_members_it_ran_off_the_record_and_no_others() {
    let mut f = Failed::default();
    f.settle(
        "",
        &names(&["a", "b", "c"]),
        &[case("a", Outcome::Fail), case("b", Outcome::Fail)],
        true,
    );
    assert!(f.clear("", &names(&["a", "c"])));
    assert_eq!(f.of(""), set(&["b"]));
    // Nothing to take off is no change, whichever way it is asked.
    assert!(!f.clear("", &names(&["a", "c"])));
    assert!(!f.clear("--features x", &names(&["b"])));
    assert_eq!(f.of(""), set(&["b"]));
    assert!(f.clear("", &names(&["b"])));
    assert_eq!(f, Failed::default());
}

/// The write goes through a file beside it, which is gone once it lands.
#[test]
fn the_record_is_written_beside_itself_and_renamed_over() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Failed::default();
    f.settle("", &names(&["a"]), &[case("a", Outcome::Fail)], true);
    f.save(dir.path()).unwrap();
    f.save(dir.path()).unwrap();
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["failed.json"]);
}
