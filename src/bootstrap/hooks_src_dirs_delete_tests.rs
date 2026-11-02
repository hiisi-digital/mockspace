//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the pre-push hook does with a push that deletes refs, and with one it is
//! fed no ref line for.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`, and the
//! table of pushes is that of `hooks_src_dirs_branch_tests`.

use super::hooks_src_dirs_branch_tests::{PUSH_TABLE, push_lines};
use super::hooks_src_dirs_tests::{KASKI, fixture, git, run_full, scope_call, stage};
use super::*;

/// The null id at each width git has, the one a deleted ref carries as its local side.
fn null_ids() -> [String; 2] {
    ["0".repeat(40), "0".repeat(64)]
}

/// A push that only deletes refs publishes no commit, so there is nothing for
/// the launcher to lint. It used to fall through to the full project scope under
/// "no crate changes" and run the whole lint pass to delete a branch. No call of
/// any kind reaches the launcher, a bare `cargo mock` (the generating run) included,
/// and a remote tip the clone never fetched changes nothing about that.
#[test]
fn a_push_that_only_deletes_refs_runs_no_lint() {
    for zeros in null_ids() {
        let width = zeros.len();
        let one = format!("(delete) {zeros} refs/heads/gone {{base}}");
        let two = format!("(delete) {zeros} refs/heads/other {{base}}");
        let unfetched = format!("(delete) {zeros} refs/tags/old {}", "1".repeat(40));
        for (what, lines) in [
            ("one deleted ref", vec![one.as_str()]),
            ("two deleted refs", vec![one.as_str(), two.as_str()]),
            ("one deleted ref at a tip never fetched", vec![unfetched.as_str()]),
            (
                "a deleted ref at a tip never fetched beside one that was",
                vec![unfetched.as_str(), one.as_str()],
            ),
        ] {
            let (out, calls) = push_lines(&[("docs/x.md", "x\n")], &lines);
            assert!(
                !out.contains("scope: full project"),
                "{what}, {width} zeros: hook said:\n{out}"
            );
            assert!(
                calls.is_empty(),
                "{what}, {width} zeros: the launcher was called: {calls:?}\n{out}"
            );
            assert!(
                out.contains("scope: none (the push deletes refs and publishes no commit)"),
                "{what}, {width} zeros: hook said:\n{out}"
            );
            assert!(
                out.contains("pre-push: validation passed."),
                "{what}, {width} zeros: hook said:\n{out}"
            );
        }
    }
}

/// The control for the arm above: a deletion beside a ref that does publish a
/// commit leaves the scope to that ref, in either order and for every shape of
/// push the table above holds, so the delete line vouches for nothing about the
/// other and takes nothing away from it.
#[test]
fn a_deleted_ref_beside_a_published_one_keeps_the_scope_of_the_published_one() {
    let zeros = "0".repeat(40);
    let gone = format!("(delete) {zeros} refs/heads/gone {{base}}");
    let pushed = "refs/heads/m {head} refs/heads/m {base}".to_string();
    for (what, files, want) in PUSH_TABLE {
        for (delete_first, lines) in [
            (true, vec![gone.as_str(), pushed.as_str()]),
            (false, vec![pushed.as_str(), gone.as_str()]),
        ] {
            let (out, calls) = push_lines(files, &lines);
            assert_eq!(
                scope_call(&calls),
                *want,
                "{what}, delete first {delete_first}: hook said:\n{out}"
            );
            assert!(
                !out.contains("scope: none"),
                "{what}: the publishing ref was skipped:\n{out}"
            );
        }
    }
}

/// A push git runs the hook for with no ref line, as it does for one that is
/// already up to date, has no range to scope by, so it takes the full scope; and
/// a blank line is no ref either. It is not a push of deletions.
#[test]
fn a_push_fed_no_ref_takes_the_full_scope() {
    for (what, lines) in [("no stdin at all", vec![]), ("a blank line", vec![""])] {
        let (out, calls) = push_lines(&[("docs/x.md", "x\n")], &lines);
        assert_eq!(
            scope_call(&calls),
            "mock --lint-only --strict",
            "{what}: hook said:\n{out}"
        );
        assert!(
            out.contains("scope: full project (no crate changes)"),
            "{what}: hook said:\n{out}"
        );
        assert!(!out.contains("scope: none"), "{what}: hook said:\n{out}");
    }
}

/// What the hook is fed by real git, which the comment ahead of its scope loop
/// relies on: a line for the first push of a commit and none for the same push
/// repeated, which git runs the hook for all the same.
#[test]
fn git_feeds_the_pre_push_hook_no_line_for_a_push_that_is_already_up_to_date() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "--bare", "remote.git"]);
    git(root, &["init", "-q", "work"]);
    let work = root.join("work");
    git(
        &work,
        &["remote", "add", "origin", root.join("remote.git").to_str().unwrap()],
    );
    stage(&work, &[("f", "one\n")]);
    git(&work, &["commit", "-q", "-m", "one"]);
    let log = root.join("stdin-lines");
    crate::test_exec::write_executable(
        &work.join(".git/hooks/pre-push"),
        format!(
            "#!/bin/sh\nwc -l | tr -d ' ' >> '{}'\nexit 0\n",
            log.display()
        ),
    );
    git(&work, &["push", "-q", "origin", "HEAD:refs/heads/m"]);
    git(&work, &["push", "-q", "origin", "HEAD:refs/heads/m"]);
    let fed: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(fed, ["1", "0"], "lines fed to the hook, one entry a push");
}

/// A push of deletions alone still ends in the repository's own hook, after the
/// gate, with the same lines: a hook guarding branch protection is exactly the
/// one a deletion has to reach.
#[test]
fn a_push_that_only_deletes_refs_still_runs_the_repositorys_own_hook() {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let own = root.join(".git/hooks/pre-push");
    crate::test_exec::write_executable(
        &own,
        "#!/usr/bin/env bash\necho \"own hook: $(cat)\"\nexit 0\n",
    );
    let line = format!(
        "(delete) {} refs/heads/gone {}",
        "0".repeat(40),
        "1".repeat(40)
    );
    let (out, calls) = run_full(
        &gen_pre_push("mock", &HookLayout::of(KASKI), &own),
        root,
        &format!("{line}\n"),
    );
    assert!(calls.is_empty(), "the launcher was called: {calls:?}\n{out}");
    let passed = out
        .find("pre-push: validation passed.")
        .unwrap_or_else(|| panic!("the gate did not pass:\n{out}"));
    let ran = out
        .find("own hook:")
        .unwrap_or_else(|| panic!("the own hook did not run:\n{out}"));
    assert!(passed < ran, "the gate runs before the own hook:\n{out}");
    assert!(
        out[ran..].contains("refs/heads/gone"),
        "the delete line was replayed to the own hook:\n{out}"
    );
}

/// The null id is tested as nothing-but-zeros, so no width of it is spelled out in
/// the generated text, in the message scan or in the scope loop.
#[test]
fn no_generated_pre_push_text_spells_the_null_id_at_one_width() {
    let hook = gen_pre_push("mock", &HookLayout::of(KASKI), Path::new("no-user-hook"));
    let scan = message_prepush_scan_body();
    for (what, text) in [("the hook", hook), ("the message scan", scan)] {
        assert!(
            !text.contains(&"0".repeat(40)),
            "{what} spells the SHA-1 null id out"
        );
    }
}
