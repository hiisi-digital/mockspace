//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two branches that both reran a member merge their test-history files, and a
//! commit that loses a green record it has no reason to is refused.
//!
//! Every scenario runs the real binary and real git in a scratch repository that
//! `cargo mock` has wired, with `cargo mock` shimmed to this build, so what is
//! asserted is what a consumer's clone does at `git merge` and at `git commit`:
//! the `.gitattributes` line, the driver in the clone's config, the
//! `merge-test-history` subcommand git calls through it, and the
//! `check-test-history` subcommand the pre-commit hook calls.
//!
//! The control carries the rest. With the driver's config removed the same two
//! branches conflict as text, which shows the fixture conflicts at all: a clean
//! merge over a fixture that never conflicted would pass for any reason.

mod common;

use std::fs;
use std::process::Output;

use common::{Repo, write};
use serde_json::{Value, json};

const APP: &str = "mock/test-history/default/app.json";
const LONE: &str = "mock/test-history/default/lone.json";

fn pkg(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n"
    )
}

fn green(tag: &str) -> Value {
    json!({ "full": tag, "own": { "app": tag } })
}

fn record(
    version: u32,
    member: &str,
    g: Option<&str>,
    tests: &[(&str, f64, bool, &str)],
) -> String {
    let mut t = serde_json::Map::new();
    for (name, secs, passed, at) in tests {
        t.insert(
            name.to_string(),
            json!({ "secs": secs, "passed": passed, "passed_at": if at.is_empty() { Value::Null } else { json!(at) } }),
        );
    }
    let v = json!({
        "version": version,
        "flavour": "default",
        "member": member,
        "green": g.map(green),
        "tests": if t.is_empty() { json!({}) } else { json!({ member: t }) },
        "bins": {},
    });
    format!("{}\n", serde_json::to_string_pretty(&v).unwrap())
}

fn app(g: Option<&str>, tests: &[(&str, f64, bool, &str)]) -> String {
    record(2, "app", g, tests)
}

fn put(r: &Repo, file: &str, text: &str) {
    write(&r.root.join(file), text);
}

fn read(r: &Repo, file: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(r.root.join(file)).unwrap()).unwrap()
}

/// A workspace of two members, `app` and `lone`, each with a record committed, on
/// `dev`, with the wiring in place.
fn repo() -> Repo {
    let r = Repo::new();
    write(
        &r.root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\", \"crates/lone\"]\nresolver = \"2\"\n",
    );
    for m in ["app", "lone"] {
        write(&r.root.join(format!("mock/crates/{m}/Cargo.toml")), &pkg(m));
        write(
            &r.root.join(format!("mock/crates/{m}/src/lib.rs")),
            "pub fn f() {}\n",
        );
    }
    write(&r.root.join(".gitignore"), "target/\n");
    put(&r, APP, &app(Some("g0"), &[("slow", 30.0, true, "b0")]));
    put(&r, LONE, &record(2, "lone", Some("l0"), &[]));
    r.generate();
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "chore: add a workspace and its history"]);
    r
}

fn commit_at(r: &Repo, when: &str, msg: &str) {
    r.git(&["add", "-A"]);
    let out = r.git_env(
        &[("GIT_COMMITTER_DATE", when), ("GIT_AUTHOR_DATE", when)],
        &["commit", "-q", "--no-verify", "-m", msg],
    );
    assert!(out.status.success(), "{}", Repo::failure(&out));
}

/// Branch `a` and `dev` each rewrite `APP`; `a` at `a_time`, `dev` at `dev_time`.
fn two_sides(r: &Repo, on_a: &str, on_dev: &str, a_time: &str, dev_time: &str) {
    r.git(&["switch", "-q", "-c", "a"]);
    put(r, APP, on_a);
    commit_at(r, a_time, "chore: rerun on a");
    r.git(&["switch", "-q", "dev"]);
    put(r, APP, on_dev);
    commit_at(r, dev_time, "chore: rerun on dev");
}

const EARLY: &str = "2026-01-01T00:00:00Z";
const LATE: &str = "2026-02-01T00:00:00Z";

fn merge(r: &Repo) -> Output {
    r.git_out(&["merge", "a", "-m", "chore: merge a"])
}

fn guard(r: &Repo) -> Output {
    r.cmd("mock").arg("check-test-history").output().unwrap()
}

#[test]
fn control_without_the_driver_two_reruns_of_one_member_conflict() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g1"), &[("slow", 31.0, true, "b1")]),
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        LATE,
    );
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-test-history"]);
    let merged = merge(&r);
    assert!(
        !merged.status.success(),
        "the fixture must conflict without the driver"
    );
    assert_eq!(r.unmerged(), [APP]);
}

#[test]
fn the_attribute_line_and_the_driver_are_installed_by_a_run() {
    let r = repo();
    let attrs = fs::read_to_string(r.root.join(".gitattributes")).unwrap();
    assert!(
        attrs
            .lines()
            .any(|l| l == "/mock/test-history/**/*.json merge=mockspace-test-history"),
        "{attrs}"
    );
    assert!(
        r.config("merge.mockspace-test-history.driver")
            .is_some_and(|d| d.contains("cargo mock merge-test-history %O %A %B %P %L")),
    );
}

#[test]
fn two_reruns_with_different_greens_merge_with_the_green_dropped_and_the_tests_unioned() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g1"), &[
            ("slow", 30.0, true, "b0"),
            ("only_a", 12.0, true, "b1"),
        ]),
        &app(Some("g2"), &[
            ("slow", 30.0, true, "b0"),
            ("only_dev", 14.0, true, "b2"),
        ]),
        EARLY,
        LATE,
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().is_empty());
    let v = read(&r, APP);
    assert_eq!(
        v["green"],
        Value::Null,
        "a green both moved is dropped: {v}"
    );
    let names: Vec<_> = v["tests"]["app"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(names, ["only_a", "only_dev", "slow"]);
    // it settles to the text a run writes, so the next run leaves it alone
    let on_disk = fs::read_to_string(r.root.join(APP)).unwrap();
    assert!(
        on_disk.ends_with("}\n") && !on_disk.contains("<<<<"),
        "{on_disk}"
    );
}

#[test]
fn a_green_only_one_side_moved_survives_the_merge() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g0"), &[
            ("slow", 30.0, true, "b0"),
            ("only_a", 12.0, true, "b1"),
        ]),
        &app(Some("g2"), &[("slow", 30.0, true, "b0")]),
        EARLY,
        LATE,
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let v = read(&r, APP);
    assert_eq!(v["green"]["full"], "g2");
    assert!(
        v["tests"]["app"]
            .as_object()
            .unwrap()
            .contains_key("only_a")
    );
}

#[test]
fn a_test_timed_on_both_sides_keeps_the_later_commits_measurement() {
    for (a_time, dev_time, expect) in [(EARLY, LATE, 40.0), (LATE, EARLY, 31.0)] {
        let r = repo();
        two_sides(
            &r,
            &app(Some("g1"), &[("slow", 31.0, true, "b1")]),
            &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
            a_time,
            dev_time,
        );
        let merged = merge(&r);
        assert!(merged.status.success(), "{}", Repo::failure(&merged));
        assert_eq!(
            read(&r, APP)["tests"]["app"]["slow"]["secs"],
            json!(expect),
            "a at {a_time}, dev at {dev_time}"
        );
    }
}

#[test]
fn a_version_one_file_meeting_a_version_two_one_stops_the_merge_with_a_message() {
    let r = repo();
    two_sides(
        &r,
        &record(1, "app", Some("g1"), &[("slow", 31.0, true, "b1")]),
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        LATE,
    );
    let merged = merge(&r);
    assert!(!merged.status.success());
    assert_eq!(r.unmerged(), [APP]);
    let said = Repo::failure(&merged);
    assert!(
        said.contains("version 1 on our side") || said.contains("version 2 on our side"),
        "{said}"
    );
    assert!(said.contains(APP), "{said}");
}

#[test]
fn a_malformed_file_stops_the_merge_with_a_message() {
    let r = repo();
    two_sides(
        &r,
        "{ this is not json",
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        LATE,
    );
    let merged = merge(&r);
    assert!(!merged.status.success());
    assert_eq!(r.unmerged(), [APP]);
    assert!(
        Repo::failure(&merged).contains("not a test-history record"),
        "{}",
        Repo::failure(&merged)
    );
}

#[test]
fn a_member_of_another_name_stops_the_merge_with_a_message() {
    let r = repo();
    two_sides(
        &r,
        &record(2, "lone", Some("g1"), &[]),
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        LATE,
    );
    let merged = merge(&r);
    assert!(!merged.status.success());
    assert!(Repo::failure(&merged).contains("one file is one member in one flavour"));
}

// ---- the commit guard ----

fn stage(r: &Repo, file: &str, text: &str) {
    put(r, file, text);
    r.git(&["add", file]);
}

#[test]
fn a_commit_that_nulls_a_green_it_has_no_reason_to_is_refused_by_member() {
    let r = repo();
    stage(&r, APP, &app(None, &[("slow", 30.0, true, "b0")]));
    stage(&r, "README.md", "unrelated\n");
    let out = guard(&r);
    assert!(!out.status.success(), "{}", Repo::failure(&out));
    let said = Repo::failure(&out);
    assert!(
        said.contains("member `app`") && said.contains(APP),
        "{said}"
    );
    assert!(!said.contains("member `lone`"), "{said}");
}

#[test]
fn the_same_commit_through_the_hook_creates_no_commit() {
    let r = repo();
    let before = r.git(&["rev-parse", "HEAD"]);
    stage(&r, APP, &app(None, &[("slow", 30.0, true, "b0")]));
    let out = r.git_out(&["commit", "-q", "-m", "chore: record a run"]);
    assert!(!out.status.success(), "{}", Repo::failure(&out));
    assert!(
        Repo::failure(&out).contains("member `app`"),
        "{}",
        Repo::failure(&out)
    );
    assert_eq!(r.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn a_commit_that_touches_the_members_inputs_may_null_its_green() {
    let r = repo();
    stage(&r, APP, &app(None, &[("slow", 30.0, true, "b0")]));
    stage(
        &r,
        "mock/crates/app/src/lib.rs",
        "pub fn f() { /* changed */ }\n",
    );
    let out = guard(&r);
    assert!(out.status.success(), "{}", Repo::failure(&out));
}

#[test]
fn a_change_to_another_member_is_not_this_members_input() {
    let r = repo();
    stage(&r, APP, &app(None, &[("slow", 30.0, true, "b0")]));
    stage(
        &r,
        "mock/crates/lone/src/lib.rs",
        "pub fn f() { /* changed */ }\n",
    );
    assert!(!guard(&r).status.success());
}

#[test]
fn a_green_replaced_by_another_is_not_a_null() {
    let r = repo();
    stage(&r, APP, &app(Some("g9"), &[("slow", 30.0, true, "b0")]));
    assert!(guard(&r).status.success());
}

#[test]
fn a_member_whose_record_is_removed_with_it_is_not_guarded() {
    let r = repo();
    r.git(&["rm", "-q", "-r", "mock/crates/lone"]);
    r.git(&["rm", "-q", LONE]);
    write(
        &r.root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"2\"\n",
    );
    r.git(&["add", "mock/Cargo.toml"]);
    assert!(guard(&r).status.success());
}

#[test]
fn a_commit_that_stages_no_history_is_not_asked() {
    let r = repo();
    stage(&r, "README.md", "unrelated\n");
    let out = guard(&r);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        Repo::failure(&out)
    );
}

fn merge_without_committing(r: &Repo) {
    let out = r.git_out(&["merge", "--no-commit", "a"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
}

#[test]
fn a_merge_commit_holding_the_drop_the_driver_made_is_not_refused() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g1"), &[("slow", 31.0, true, "b1")]),
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        LATE,
    );
    merge_without_committing(&r);
    assert_eq!(read(&r, APP)["green"], Value::Null);
    let out = guard(&r);
    assert!(out.status.success(), "{}", Repo::failure(&out));
}

#[test]
fn a_merge_commit_that_nulls_a_green_both_sides_agree_on_is_refused() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g1"), &[
            ("slow", 30.0, true, "b0"),
            ("only_a", 12.0, true, "b1"),
        ]),
        &app(Some("g1"), &[
            ("slow", 30.0, true, "b0"),
            ("only_dev", 14.0, true, "b2"),
        ]),
        EARLY,
        LATE,
    );
    merge_without_committing(&r);
    assert_eq!(
        read(&r, APP)["green"]["full"],
        "g1",
        "the driver keeps the agreed green"
    );
    // a run cut short in the middle of the merge nulls it
    stage(&r, APP, &app(None, &[("slow", 30.0, true, "b0")]));
    let out = guard(&r);
    assert!(!out.status.success(), "{}", Repo::failure(&out));
    assert!(
        Repo::failure(&out).contains("member `app`"),
        "{}",
        Repo::failure(&out)
    );
}

// ---- which side measured later, and the other ways a branch is brought in ----

const MID: &str = "2026-01-15T00:00:00Z";

fn commit_file_at(r: &Repo, file: &str, text: &str, when: &str, msg: &str) {
    put(r, file, text);
    commit_at(r, when, msg);
}

fn slow_secs(r: &Repo) -> serde_json::Value {
    read(r, APP)["tests"]["app"]["slow"]["secs"].clone()
}

/// The trunk measured `slow` in January and has had unrelated commits since; the
/// branch measured it in mid January. The branch's measurement is the newer, and
/// a trunk tip committed in March says nothing about when the file was.
#[test]
fn the_later_measurement_is_the_file_last_changed_later_not_the_later_tip() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    commit_file_at(
        &r,
        APP,
        &app(Some("g1"), &[("slow", 31.0, true, "b1")]),
        MID,
        "chore: rerun on a",
    );
    r.git(&["switch", "-q", "dev"]);
    commit_file_at(
        &r,
        APP,
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        EARLY,
        "chore: rerun on dev",
    );
    commit_file_at(
        &r,
        "notes.txt",
        "later notes\n",
        LATE,
        "chore: touch something else",
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(
        slow_secs(&r),
        json!(31.0),
        "the branch measured it later than the trunk did"
    );
}

/// The same fixture with the sides swapped, so that the answer is not "theirs".
#[test]
fn the_file_last_changed_later_wins_from_either_side() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    commit_file_at(
        &r,
        APP,
        &app(Some("g1"), &[("slow", 31.0, true, "b1")]),
        EARLY,
        "chore: rerun on a",
    );
    commit_file_at(
        &r,
        "notes.txt",
        "later notes\n",
        LATE,
        "chore: touch something else",
    );
    r.git(&["switch", "-q", "dev"]);
    commit_file_at(
        &r,
        APP,
        &app(Some("g2"), &[("slow", 40.0, true, "b2")]),
        MID,
        "chore: rerun on dev",
    );
    let merged = merge(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(slow_secs(&r), json!(40.0));
}

/// `a` timed `slow` at 40s on a January commit and the trunk at 31s on a later
/// one, so a clock that is known answers 31 and one that is not answers the
/// longer, 40.
fn clocked_sides(r: &Repo) {
    two_sides(
        r,
        &app(Some("g1"), &[
            ("slow", 40.0, true, "b1"),
            ("only_a", 12.0, true, "b1"),
        ]),
        &app(Some("g2"), &[
            ("slow", 31.0, true, "b2"),
            ("only_dev", 14.0, true, "b2"),
        ]),
        EARLY,
        LATE,
    );
}

fn names(r: &Repo) -> Vec<String> {
    read(r, APP)["tests"]["app"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn a_rebase_merges_the_two_files_and_drops_the_green_both_moved() {
    let r = repo();
    clocked_sides(&r);
    r.git(&["switch", "-q", "a"]);
    let out = r.git_out(&["rebase", "dev"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert!(r.unmerged().is_empty());
    let v = read(&r, APP);
    assert_eq!(v["green"], Value::Null, "{v}");
    assert_eq!(names(&r), ["only_a", "only_dev", "slow"]);
    // a rebase has no second tip to ask the clock about, so the longer timing wins
    assert_eq!(slow_secs(&r), json!(40.0));
}

#[test]
fn a_cherry_pick_merges_the_two_files_and_drops_the_green_both_moved() {
    let r = repo();
    clocked_sides(&r);
    let out = r.git_out(&["cherry-pick", "a"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert!(r.unmerged().is_empty());
    let v = read(&r, APP);
    assert_eq!(v["green"], Value::Null, "{v}");
    assert_eq!(names(&r), ["only_a", "only_dev", "slow"]);
    assert_eq!(slow_secs(&r), json!(40.0));
}

#[test]
fn a_rebase_keeps_a_green_only_the_replayed_commit_moved() {
    let r = repo();
    two_sides(
        &r,
        &app(Some("g1"), &[
            ("slow", 30.0, true, "b0"),
            ("only_a", 12.0, true, "b1"),
        ]),
        &app(Some("g0"), &[
            ("slow", 30.0, true, "b0"),
            ("only_dev", 14.0, true, "b2"),
        ]),
        EARLY,
        LATE,
    );
    r.git(&["switch", "-q", "a"]);
    let out = r.git_out(&["rebase", "dev"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert_eq!(read(&r, APP)["green"]["full"], "g1");
}

/// `git merge --squash` leaves no `MERGE_HEAD`, so the commit that follows is
/// told nothing of the merge. The driver still ran at the squash and dropped the
/// green both sides moved.
#[test]
fn a_squash_runs_the_driver_and_drops_the_green_both_moved() {
    let r = repo();
    clocked_sides(&r);
    let out = r.git_out(&["-c", "merge.ff=true", "merge", "--squash", "a"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert!(r.unmerged().is_empty());
    assert_eq!(read(&r, APP)["green"], Value::Null);
    assert_eq!(names(&r), ["only_a", "only_dev", "slow"]);
}

/// A squash whose branch changed nothing the member is built from nulls a green
/// the trunk's own record still describes, since the tree it lands is the trunk's.
/// The guard has no merge to excuse the null and refuses it, and that is right:
/// the way out it names, keeping the staged record out, keeps a true green.
#[test]
fn a_squash_that_nulls_a_green_over_an_unchanged_member_is_refused() {
    let r = repo();
    clocked_sides(&r);
    let out = r.git_out(&["-c", "merge.ff=true", "merge", "--squash", "a"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    let refused = guard(&r);
    assert!(!refused.status.success(), "{}", Repo::failure(&refused));
    let said = Repo::failure(&refused);
    assert!(
        said.contains("member `app`") && said.contains("changes nothing its tests are built from"),
        "{said}"
    );
    // restoring the record the trunk holds is what the message asks for
    r.git(&["restore", "--staged", "--worktree", "--source=HEAD", APP]);
    assert_eq!(read(&r, APP)["green"]["full"], "g2");
    assert!(guard(&r).status.success());
}

/// The same squash over a branch that did change the member is accepted, because
/// a commit touching a member's inputs owes it a new run and may drop its green.
#[test]
fn a_squash_that_nulls_a_green_over_a_changed_member_is_accepted() {
    let r = repo();
    r.git(&["switch", "-q", "-c", "a"]);
    put(&r, "mock/crates/app/src/lib.rs", "pub fn f() { /* a */ }\n");
    put(&r, APP, &app(Some("g1"), &[("slow", 40.0, true, "b1")]));
    commit_at(&r, EARLY, "chore: change app and rerun it");
    r.git(&["switch", "-q", "dev"]);
    commit_file_at(
        &r,
        APP,
        &app(Some("g2"), &[("slow", 31.0, true, "b2")]),
        LATE,
        "chore: rerun on dev",
    );
    let out = r.git_out(&["-c", "merge.ff=true", "merge", "--squash", "a"]);
    assert!(out.status.success(), "{}", Repo::failure(&out));
    assert_eq!(read(&r, APP)["green"], Value::Null);
    let g = guard(&r);
    assert!(g.status.success(), "{}", Repo::failure(&g));
}
