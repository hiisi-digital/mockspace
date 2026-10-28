//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn green(tag: &str) -> Green {
    Green {
        full: tag.to_string(),
        own:  BTreeMap::new(),
    }
}

/// `app` depends on `core`; `lone` on nothing. `app` reads `content/` from outside.
fn inputs() -> Inputs {
    let dirs = BTreeMap::from([
        ("app".to_string(), PathBuf::from("mock/crates/app")),
        ("core".to_string(), PathBuf::from("mock/crates/core")),
        ("lone".to_string(), PathBuf::from("mock/crates/lone")),
    ]);
    Inputs {
        dirs,
        closure: BTreeMap::from([
            ("app".to_string(), BTreeSet::from(["core".to_string()])),
            ("core".to_string(), BTreeSet::new()),
            ("lone".to_string(), BTreeSet::new()),
        ]),
        reaches: BTreeMap::from([("app".to_string(), vec![PathBuf::from("content")])]),
        workspace: vec![PathBuf::from("mock/Cargo.lock"), PathBuf::from("Cargo.toml")],
    }
}

fn changed(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

fn plain(member: &str) -> Nulled {
    Nulled {
        path:   format!("mock/test-history/default/{member}.json"),
        member: member.to_string(),
        merge:  None,
    }
}

fn merging(member: &str, base: Option<&str>, ours: Option<&str>, theirs: Option<&str>) -> Nulled {
    Nulled {
        merge: Some(Sides {
            base:   base.map(green),
            ours:   ours.map(green),
            theirs: theirs.map(green),
        }),
        ..plain(member)
    }
}

fn judged(nulled: &[Nulled], paths: &[&str]) -> Vec<Refusal> {
    judge(nulled, &changed(paths), &mut || Ok(inputs())).unwrap()
}

#[test]
fn a_member_no_staged_path_belongs_to_is_refused_by_name() {
    let r = judged(&[plain("app")], &[
        "mock/crates/lone/src/lib.rs",
        "README.md",
    ]);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].member.as_str(), r[0].why), ("app", UNTOUCHED));
    assert_eq!(r[0].path, "mock/test-history/default/app.json");
}

#[test]
fn a_commit_with_nothing_else_in_it_touches_nothing() {
    assert_eq!(judged(&[plain("app")], &[]).len(), 1);
}

#[test]
fn the_members_own_directory_is_its_input() {
    assert!(judged(&[plain("app")], &["mock/crates/app/tests/x.rs"]).is_empty());
    // a sibling directory sharing a prefix of the name is not
    assert_eq!(
        judged(&[plain("app")], &["mock/crates/app-extra/src/lib.rs"]).len(),
        1
    );
}

#[test]
fn a_member_compiled_into_its_tests_is_an_input_and_the_other_way_round_is_not() {
    assert!(judged(&[plain("app")], &["mock/crates/core/src/lib.rs"]).is_empty());
    assert_eq!(
        judged(&[plain("core")], &["mock/crates/app/src/lib.rs"]).len(),
        1
    );
}

#[test]
fn what_its_sources_read_from_outside_is_an_input_for_that_member_only() {
    assert!(judged(&[plain("app")], &["content/rows.txt"]).is_empty());
    assert_eq!(judged(&[plain("lone")], &["content/rows.txt"]).len(), 1);
}

#[test]
fn a_workspace_wide_file_touches_every_member() {
    for m in ["app", "core", "lone"] {
        assert!(judged(&[plain(m)], &["mock/Cargo.lock"]).is_empty(), "{m}");
        assert!(judged(&[plain(m)], &["Cargo.toml"]).is_empty(), "{m}");
    }
}

#[test]
fn a_member_the_workspace_no_longer_has_is_not_guarded() {
    assert!(judged(&[plain("removed")], &["README.md"]).is_empty());
}

#[test]
fn only_the_members_that_lost_a_green_are_named() {
    let r = judged(&[plain("app"), plain("lone"), plain("core")], &[
        "mock/crates/core/src/lib.rs",
    ]);
    // core and app (compiled with core) are touched; lone is not
    assert_eq!(r.iter().map(|r| r.member.as_str()).collect::<Vec<_>>(), [
        "lone"
    ]);
}

#[test]
fn the_inputs_are_read_only_when_a_null_needs_them() {
    let mut asked = 0;
    let merge_drop = merging("app", Some("g0"), Some("g1"), Some("g2"));
    let r = judge(&[merge_drop], &[], &mut || {
        asked += 1;
        Ok(inputs())
    })
    .unwrap();
    assert!(r.is_empty());
    assert_eq!(asked, 0, "a drop the driver makes needs no inputs");
}

#[test]
fn an_unreadable_workspace_is_an_error_not_a_pass() {
    let r = judge(&[plain("app")], &[], &mut || {
        Err("cargo metadata failed".into())
    });
    assert_eq!(r.unwrap_err(), "cargo metadata failed");
}

#[test]
fn a_merge_that_dropped_a_green_both_sides_moved_is_not_a_loss() {
    // both moved it from g0, to different places: the driver's own drop
    let r = judged(&[merging("lone", Some("g0"), Some("g1"), Some("g2"))], &[]);
    assert!(r.is_empty());
    // added on both sides with no base, different
    assert!(judged(&[merging("lone", None, Some("g1"), Some("g2"))], &[]).is_empty());
}

#[test]
fn a_merge_that_nulls_a_green_the_driver_would_have_kept_is_refused() {
    // the sides agree
    let same = judged(&[merging("lone", Some("g0"), Some("g1"), Some("g1"))], &[]);
    assert_eq!(same.len(), 1);
    assert_eq!(same[0].why, KEPT_BY_THE_DRIVER);
    // only theirs moved it
    assert_eq!(
        judged(&[merging("lone", Some("g0"), Some("g0"), Some("g1"))], &[]).len(),
        1
    );
    // only ours moved it
    assert_eq!(
        judged(&[merging("lone", Some("g0"), Some("g1"), Some("g0"))], &[]).len(),
        1
    );
}

#[test]
fn a_merge_that_touched_the_inputs_may_null_what_the_driver_would_have_kept() {
    let r = judged(&[merging("app", Some("g0"), Some("g1"), Some("g1"))], &[
        "mock/crates/core/src/lib.rs",
    ]);
    assert!(r.is_empty());
}

#[test]
fn the_drop_rule_over_every_triple_decides_a_merge_null() {
    let vals = [None, Some("a"), Some("b")];
    for b in vals {
        for o in vals {
            for t in vals {
                let refused = !judged(&[merging("lone", b, o, t)], &[]).is_empty();
                // a null in a commit that touched nothing is accepted exactly where the
                // driver's rule gives none
                let would_null = if o == t {
                    o.is_none()
                } else if o == b {
                    t.is_none()
                } else if t == b {
                    o.is_none()
                } else {
                    true
                };
                assert_eq!(!refused, would_null, "base {b:?} ours {o:?} theirs {t:?}");
            }
        }
    }
}

/// A file moved out of one member into another changes both, so the member it
/// left is not judged untouched. With git's rename detection on, the listing
/// names the destination alone.
#[test]
fn a_file_moved_out_of_a_member_lists_the_member_it_left() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    run(&["init", "-q"]);
    let body = "pub fn a() -> u32 {\n    let x = 1;\n    let y = 2;\n    x + y\n}\n\npub fn b() {}\n";
    std::fs::create_dir_all(root.join("mock/crates/app/src")).unwrap();
    std::fs::create_dir_all(root.join("mock/crates/core/src")).unwrap();
    std::fs::write(root.join("mock/crates/app/src/x.rs"), body).unwrap();
    run(&["add", "-A"]);
    run(&["commit", "-q", "-m", "base"]);
    run(&["mv", "mock/crates/app/src/x.rs", "mock/crates/core/src/x.rs"]);
    // the control: git does read this as a rename
    let status = git(root, &["diff", "--cached", "--name-status", "-M", "HEAD"]).unwrap();
    assert!(String::from_utf8_lossy(&status).starts_with("R100"), "not a rename");
    let mut got = staged_changes(root, "mock/test-history");
    got.sort();
    assert_eq!(got, changed(&["mock/crates/app/src/x.rs", "mock/crates/core/src/x.rs"]));
}
