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

/// A save by a run that covered `members`, in a workspace of just them.
fn save(h: &History, mock: &Path, members: &[&str]) -> Result<(), String> {
    let members: Vec<String> = members.iter().map(|m| m.to_string()).collect();
    h.save(mock, &Scope {
        flavour:   "",
        ran:       &members,
        workspace: &members,
    })
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
#[test]
fn a_flavour_names_its_directory() {
    assert_eq!(flavour_dir(""), "default");
    assert_eq!(flavour_dir("--features editor"), "features-editor");
    assert_eq!(
        flavour_dir("--release --features a,b"),
        "release-features-a-b"
    );
}

fn live(entries: &[(&str, &[&str])]) -> crate::suite::live::Live {
    entries
        .iter()
        .map(|(b, ts)| (b.to_string(), ts.iter().map(|t| t.to_string()).collect()))
        .collect()
}

fn remembered(h: &mut History, bin: &str, name: &str) {
    h.record_cases("", &[case(bin, name, 40.0, Outcome::Pass)], &fps(), T);
}

#[test]
fn a_gone_binary_is_forgotten_and_a_present_one_keeps_its_entry() {
    let mut h = History::default();
    remembered(&mut h, "village", "drawn");
    remembered(&mut h, "village::example/figure_preview", "draws");
    h.prune(
        "",
        &["village".to_string()],
        &live(&[("village", &["drawn"])]),
    );
    let t = &h.flavours[""].tests;
    assert!(t.contains_key("village"), "{t:?}");
    assert!(!t.contains_key("village::example/figure_preview"), "{t:?}");
}

#[test]
fn a_gone_test_in_a_present_binary_is_forgotten() {
    let mut h = History::default();
    remembered(&mut h, "village", "drawn");
    remembered(&mut h, "village", "renamed_away");
    h.prune(
        "",
        &["village".to_string()],
        &live(&[("village", &["drawn"])]),
    );
    let t = &h.flavours[""].tests["village"];
    assert!(
        t.contains_key("drawn") && !t.contains_key("renamed_away"),
        "{t:?}"
    );
}

#[test]
fn members_that_did_not_run_are_left_alone() {
    let mut h = History::default();
    remembered(&mut h, "mesh", "drawn");
    h.prune("", &["village".to_string()], &live(&[("village", &[])]));
    assert!(h.flavours[""].tests.contains_key("mesh"));
}

/// A record written before binaries were kept has no field for them and reads
/// as having none, rather than as unreadable. (Its version is another matter:
/// the fingerprints in it were made of other inputs.)
#[test]
fn a_file_without_binaries_reads_as_having_none() {
    let mock = scratch("nobins");
    let dir = History::dir(&mock).join("default");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.json"),
        format!(r#"{{"version": {VERSION}, "flavour": "", "member": "a", "green": null, "tests": {{}}}}"#),
    )
    .unwrap();
    std::fs::write(
        dir.join("b.json"),
        r#"{"version": 1, "flavour": "", "member": "b", "green": {"full": "x", "own": {}}, "tests": {}}"#,
    )
    .unwrap();
    let h = History::load(&mock);
    let f = h.flavour("").expect("the current file was read");
    assert!(f.bins.is_empty());
    assert!(
        !f.green.contains_key("b"),
        "a version 1 record is made of other fingerprints and is not read"
    );
    std::fs::remove_dir_all(&mock).ok();
}

#[test]
fn a_gone_binary_loses_its_pass_and_a_present_one_keeps_it() {
    let mut h = History::default();
    let f = h.flavour_mut("");
    f.bins.insert("village".to_string(), "a".to_string());
    f.bins.insert("village::gone".to_string(), "b".to_string());
    f.bins.insert("village::kept".to_string(), "c".to_string());
    f.bins.insert("other::gone".to_string(), "d".to_string());
    h.prune(
        "",
        &["village".to_string()],
        &live(&[("village", &[]), ("village::kept", &[])]),
    );
    let keys: Vec<_> = h.flavour("").unwrap().bins.keys().cloned().collect();
    assert_eq!(
        keys,
        vec!["other::gone", "village", "village::kept"],
        "a member not listed is left as it is"
    );
}

/// A test's pass stands at its own binary's fingerprint where one is given, and
/// at its member's where it is not.
#[test]
fn a_tests_pass_is_recorded_at_its_binarys_fingerprint() {
    let at = BTreeMap::from([
        ("render".to_string(), "member".to_string()),
        ("render::it".to_string(), "binary".to_string()),
    ]);
    let mut h = History::default();
    h.record_cases(
        "",
        &[
            case("render::it", "slow", 40.0, Outcome::Pass),
            case("render", "other_slow", 40.0, Outcome::Pass),
            case("render::example/e", "third", 40.0, Outcome::Pass),
        ],
        &at,
        T,
    );
    let t = &h.flavour("").unwrap().tests;
    assert_eq!(t["render::it"]["slow"].passed_at.as_deref(), Some("binary"));
    assert_eq!(
        t["render"]["other_slow"].passed_at.as_deref(),
        Some("member")
    );
    assert_eq!(
        t["render::example/e"]["third"].passed_at.as_deref(),
        Some("member")
    );
}

/// A version 1 record keeps what it timed, so the first run after the bump still
/// knows which tests are heavy and holds them to `heavy_threads`, and gives up
/// every pass, which was made at fingerprints of other inputs.
#[test]
fn a_version_1_record_gives_its_timings_and_no_pass() {
    let mock = scratch("v1");
    let dir = History::dir(&mock).join("default");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("render.json"),
        r#"{"version": 1, "flavour": "", "member": "render",
            "green": {"full": "old", "own": {"render": "old"}},
            "tests": {"render::it": {
                "drawn": {"secs": 40.0, "passed": true, "passed_at": "old"},
                "broke": {"secs": 12.0, "passed": false, "passed_at": null}}}}"#,
    )
    .unwrap();
    let h = History::load(&mock);
    let f = h.flavour("").expect("the v1 file was read");
    assert!(
        f.green.is_empty(),
        "no member is green on the strength of a v1 pass"
    );
    assert!(f.bins.is_empty());
    let t = &f.tests["render::it"];
    assert_eq!(t["drawn"].secs, 40.0);
    assert!(t["drawn"].passed, "that it passed is kept, for --cheap");
    assert_eq!(t["drawn"].passed_at, None, "at what, is not");
    assert!(!t["broke"].passed);
    assert_eq!(h.heavy("", T), vec![
        ("render::it".to_string(), "broke".to_string()),
        ("render::it".to_string(), "drawn".to_string())
    ]);
    // The next save writes it as version 2.
    save(&h, &mock, &["render"]).unwrap();
    let text = std::fs::read_to_string(dir.join("render.json")).unwrap();
    assert!(text.contains(&format!("\"version\": {VERSION}")), "{text}");
    std::fs::remove_dir_all(&mock).ok();
}
