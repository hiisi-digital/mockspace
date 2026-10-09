//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const T: f64 = 10.0;

fn case(bin: &str, name: &str, secs: f64, outcome: Outcome) -> Case {
    Case {
        binary: bin.to_string(),
        name: name.to_string(),
        secs,
        outcome,
    }
}

fn fps() -> BTreeMap<String, String> {
    BTreeMap::from([("render".to_string(), "fp1".to_string())])
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "mockspace-suite-hist-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_pass_records_its_fingerprint_and_a_failure_clears_it() {
    let mut h = History::default();
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    let r = &h.flavours[""].tests["render"]["drawn"];
    assert_eq!(
        (r.secs, r.passed, r.passed_at.as_deref()),
        (40.0, true, Some("fp1"))
    );

    h.record_cases(
        "",
        &[case("render", "drawn", 41.0, Outcome::Fail)],
        &fps(),
        T,
    );
    let r = &h.flavours[""].tests["render"]["drawn"];
    assert_eq!((r.passed, r.passed_at.as_deref()), (false, None));
}

/// An ignored test did not run, and what it took to not run is no timing.
#[test]
fn a_skipped_case_leaves_the_record_alone() {
    let mut h = History::default();
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.record_cases(
        "",
        &[case("render", "drawn", 0.0, Outcome::Skipped)],
        &fps(),
        T,
    );
    assert_eq!(h.flavours[""].tests["render"]["drawn"].secs, 40.0);
}

/// Nothing is decided about a quick test, so the tracked files do not carry
/// one, and a test that got quick drops out.
#[test]
fn a_quick_test_is_not_kept_and_one_that_got_quick_drops_out() {
    let mut h = History::default();
    h.record_cases(
        "",
        &[
            case("render", "quick", 0.01, Outcome::Pass),
            case("render", "near", 6.0, Outcome::Pass),
            case("render", "drawn", 40.0, Outcome::Pass),
        ],
        &fps(),
        T,
    );
    let kept: Vec<&String> = h.flavours[""].tests["render"].keys().collect();
    assert_eq!(kept, vec!["drawn", "near"]);

    h.record_cases(
        "",
        &[case("render", "drawn", 0.5, Outcome::Pass)],
        &fps(),
        T,
    );
    let kept: Vec<&String> = h.flavours[""].tests["render"].keys().collect();
    assert_eq!(kept, vec!["near"]);
}

/// A rerun a little faster or slower rewrites nothing; crossing the threshold
/// or moving by more than half does.
#[test]
fn a_timing_is_rewritten_only_when_it_says_something_new() {
    let mut h = History::default();
    let secs = |h: &History| h.flavours[""].tests["render"]["drawn"].secs;
    h.record_cases(
        "",
        &[case("render", "drawn", 40.04, Outcome::Pass)],
        &fps(),
        T,
    );
    assert_eq!(secs(&h), 40.0, "kept to a tenth");
    h.record_cases(
        "",
        &[case("render", "drawn", 52.0, Outcome::Pass)],
        &fps(),
        T,
    );
    assert_eq!(secs(&h), 40.0, "within half again");
    h.record_cases(
        "",
        &[case("render", "drawn", 70.0, Outcome::Pass)],
        &fps(),
        T,
    );
    assert_eq!(secs(&h), 70.0, "more than half again");
    h.record_cases(
        "",
        &[case("render", "drawn", 9.0, Outcome::Pass)],
        &fps(),
        T,
    );
    assert_eq!(secs(&h), 9.0, "crossed the threshold");
}

#[test]
fn heavy_is_at_or_over_the_threshold_and_borrows_across_flavours() {
    let mut h = History::default();
    h.record_cases(
        "",
        &[
            case("render", "drawn", 40.0, Outcome::Pass),
            case("render", "edge", 10.0, Outcome::Pass),
            case("render", "near", 6.0, Outcome::Pass),
        ],
        &fps(),
        T,
    );
    let names = |v: Vec<(String, String)>| v.into_iter().map(|(_, n)| n).collect::<Vec<_>>();
    assert_eq!(names(h.heavy("", T)), vec!["drawn", "edge"]);
    // The editor flavour has never run; it borrows the default's timings.
    assert_eq!(names(h.heavy("--features editor", T)), vec![
        "drawn", "edge"
    ]);
    // Once it has run, its own timing decides, here that `drawn` got quicker.
    h.record_cases(
        "--features editor",
        &[case("render", "drawn", 6.0, Outcome::Pass)],
        &fps(),
        T,
    );
    assert_eq!(names(h.heavy("--features editor", T)), vec!["edge"]);
}

/// The record is tracked files: one per member per flavour, loaded back whole.
#[test]
fn a_saved_history_loads_back_from_one_file_per_member_and_flavour() {
    let mock = scratch("roundtrip");
    let mut h = History::load(&mock);
    assert_eq!(h.version, VERSION);
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.record_cases(
        "--features editor",
        &[case("web::it", "drawn", 30.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.flavour_mut("").green.insert("mesh".to_string(), Green {
        full: "m1".to_string(),
        own:  BTreeMap::from([("mesh".to_string(), "o1".to_string())]),
    });
    h.save(&mock).unwrap();

    let dir = History::dir(&mock);
    for f in ["default/render.json", "default/mesh.json", "features-editor/web.json"] {
        assert!(dir.join(f).is_file(), "{f} was not written");
    }
    assert_eq!(History::load(&mock).flavours, h.flavours);
    std::fs::remove_dir_all(&mock).ok();
}

/// A run that settled nothing leaves every file as it was, so a rerun does not
/// show up as a change to commit.
#[test]
fn saving_what_is_already_there_writes_nothing() {
    let mock = scratch("still");
    let mut h = History::default();
    h.record_cases(
        "",
        &[case("render", "drawn", 40.0, Outcome::Pass)],
        &fps(),
        T,
    );
    h.save(&mock).unwrap();
    let file = History::dir(&mock).join("default/render.json");
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut again = History::load(&mock);
    again.record_cases(
        "",
        &[case("render", "drawn", 41.0, Outcome::Pass)],
        &fps(),
        T,
    );
    again.save(&mock).unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
    std::fs::remove_dir_all(&mock).ok();
}

/// A member whose green a failure took away must not load it back from the
/// file it was saved in.
#[test]
fn a_member_with_nothing_left_loses_its_file() {
    let mock = scratch("stale");
    let mut h = History::default();
    h.flavour_mut("")
        .green
        .insert("mesh".to_string(), Green::default());
    h.save(&mock).unwrap();
    h.flavour_mut("").green.remove("mesh");
    h.save(&mock).unwrap();
    assert!(
        History::load(&mock)
            .flavour("")
            .is_none_or(|f| f.green.is_empty())
    );
    std::fs::remove_dir_all(&mock).ok();
}

#[test]
fn a_file_of_another_version_or_none_is_passed_over() {
    let mock = scratch("foreign");
    let dir = History::dir(&mock).join("default");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.json"),
        r#"{"version": 999, "flavour": "", "member": "a"}"#,
    )
    .unwrap();
    std::fs::write(dir.join("b.json"), "not json").unwrap();
    assert!(History::load(&mock).flavours.is_empty());
    std::fs::remove_dir_all(&mock).ok();
}

#[test]
fn a_flavour_names_its_directory() {
    assert_eq!(flavour_dir(""), "default");
    assert_eq!(flavour_dir("--features editor"), "features-editor");
    assert_eq!(
        flavour_dir("--release --features a,b"),
        "release-features-a-b"
    );
}
