//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::ffi::OsStr;

use super::*;

/// The ordinary case, and the one the whole change is for.
#[test]
fn nothing_in_the_way_means_the_roots_share_one_directory() {
    assert!(may_share(&[], None));
    assert!(may_share(&["--nocapture"], None));
}

/// Deleting this check leaves a machine that already shares one build
/// directory with two of them, which is the duplication this exists to
/// remove, arrived at from the other side.
#[test]
fn an_inherited_target_dir_is_left_alone() {
    assert!(!may_share(&[], Some(OsStr::new("/somewhere/shared"))));
    assert!(!may_share(&["--nocapture"], Some(OsStr::new("/elsewhere"))));
}

/// Set but empty is read as set. Cargo's own handling of an empty value is
/// not something this can observe, and declining to pass a flag is the
/// reading that cannot break anything: the worst it costs is the
/// duplication that was there before.
#[test]
fn an_empty_inherited_value_still_counts_as_set() {
    assert!(!may_share(&[], Some(OsStr::new(""))));
}

#[test]
fn the_callers_own_flag_wins_in_both_spellings() {
    assert!(!may_share(&["--target-dir", "/mine"], None));
    assert!(!may_share(&["--target-dir=/mine"], None));
    assert!(!may_share(&["--release", "--target-dir", "/mine"], None));
}

/// After `--` the tokens are the test binary's and say nothing about where
/// cargo builds, so one there must not suppress the shared directory.
/// Without the `take_while` this arm is the one that fails.
#[test]
fn the_same_flag_past_the_separator_is_not_cargos() {
    assert!(may_share(&["--", "--target-dir"], None));
    assert!(may_share(&["--", "--target-dir=/not-cargos"], None));
    assert!(may_share(&["--nocapture", "--", "--target-dir"], None));
}

/// One before and one after: the one before is cargo's and decides it.
#[test]
fn a_flag_on_each_side_of_the_separator_is_decided_by_the_first() {
    assert!(!may_share(
        &["--target-dir", "/mine", "--", "--target-dir"],
        None
    ));
}

/// `--target` selects a triple and is not this flag. A prefix test written
/// without the `=` would swallow it, and cross-compiling would silently
/// lose the shared directory.
#[test]
fn a_neighbouring_flag_is_not_mistaken_for_it() {
    assert!(may_share(&["--target", "x86_64-unknown-linux-gnu"], None));
    assert!(may_share(&["--target-dirty-is-not-a-flag"], None));
}

#[test]
fn a_bare_separator_alone_changes_nothing() {
    assert!(may_share(&["--"], None));
}

/// Cargo honours this spelling too, and reading only the first one leaves
/// a machine that has already chosen a build directory getting a second.
#[test]
fn the_build_spelling_of_the_variable_counts_as_set() {
    // The value `inherited_target_dir` would have found, either way round.
    assert!(!may_share(
        &[],
        Some(OsStr::new("/set/via/cargo_build_target_dir"))
    ));
}

/// The report counts what ran, and the benches tree runs outside `trees`.
/// Before this, seven trees plus benches printed `7 tree(s) green`, and a
/// benches-only repository printed `1 of 0 tree(s) failed`.
#[test]
fn the_report_counts_the_benches_tree_it_also_ran() {
    assert_eq!(ran_count(7, true), 8);
    assert_eq!(ran_count(7, false), 7);
    assert_eq!(ran_count(0, true), 1);
    assert_eq!(ran_count(0, false), 0);
}

fn tree(what: &'static str, own_root: bool) -> Tree {
    Tree {
        what,
        dir: PathBuf::from("/nowhere"),
        because: "fixture",
        own_root,
        members: false,
    }
}

/// The property the whole change is for, which nothing asserted until a
/// reviewer found a member tool being redirected into a second build tree.
#[test]
fn only_a_tree_that_is_its_own_root_is_redirected() {
    let shared = PathBuf::from("/shared");
    assert_eq!(
        redirect_for(&tree("tool", true), Some(&shared)),
        Some(&shared)
    );
    assert_eq!(
        redirect_for(&tree("lints", true), Some(&shared)),
        Some(&shared)
    );
    assert_eq!(
        redirect_for(&tree("workspace members", false), Some(&shared)),
        None
    );
    // A tool the workspace lists in `members` reaches this with
    // `own_root == false`, and its default is already `mock/target/`.
    assert_eq!(redirect_for(&tree("tool", false), Some(&shared)), None);
}

/// Deleting the `may_share` guard must not leak a directory in through the
/// per-tree half, so the two halves are asserted independently.
#[test]
fn no_shared_directory_means_no_redirect_for_anyone() {
    assert_eq!(redirect_for(&tree("tool", true), None), None);
    assert_eq!(redirect_for(&tree("workspace members", false), None), None);
}

/// `[workspace]` in the crate's own manifest is the whole test, and a
/// crate without one is not a root however it got past `is_orphaned`.
#[test]
fn a_root_is_the_manifest_declaring_a_workspace_and_nothing_else() {
    let tmp = std::env::temp_dir().join(format!(
        "mockspace-test-own-root-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let root = tmp.join("is-a-root");
    let member = tmp.join("is-a-member");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&member).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"r\"\n",
    )
    .unwrap();
    std::fs::write(member.join("Cargo.toml"), "[package]\nname = \"m\"\n").unwrap();

    assert!(declares_its_own_workspace(&root));
    assert!(!declares_its_own_workspace(&member));
    // A directory with no manifest at all reads as not a root rather than
    // panicking, which is what `is_orphaned` has always relied on.
    assert!(!declares_its_own_workspace(&tmp.join("nothing-here")));

    // The four a substring match gets wrong. The first two are the ones
    // that bite: a manifest naming the table in a comment or in prose was
    // handed a shared target directory it must not have, and nothing said
    // so, because the answer looks the same either way from outside.
    let cases: [(&str, bool, &str); 4] = [
        (
            "# [workspace] was removed when this became a member\n[package]\nname = \"c\"\n",
            false,
            "a commented-out table is not a declaration",
        ),
        (
            "[package]\nname = \"d\"\ndescription = \"how [workspace] roots behave\"\n",
            false,
            "the word inside a string is not a declaration",
        ),
        (
            "[workspace.package]\nversion = \"0.1.0\"\n\n[package]\nname = \"e\"\n",
            true,
            "a dotted workspace table defines the key just as the bare one does, \
             and cargo reads the manifest as a root either way",
        ),
        (
            "[package\nname = \"f\"\n",
            false,
            "a manifest that does not parse is not a root, which is the answer \
             cargo gives it too",
        ),
    ];
    for (i, (text, want, why)) in cases.iter().enumerate() {
        let dir = tmp.join(format!("case-{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), text).unwrap();
        assert_eq!(
            declares_its_own_workspace(&dir),
            *want,
            "{why}; manifest was {text:?}"
        );
    }

    std::fs::remove_dir_all(&tmp).ok();
}
