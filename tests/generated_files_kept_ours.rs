//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A generated file whose one side the merge driver kept is stale until a
//! regeneration has been checked, and nothing may commit or push it meanwhile.
//!
//! The driver decides at merge time that a later hook will regenerate, and
//! writes that down in `mockspace-kept-ours`. The cases below are the ways the
//! promise breaks that the driver cannot see: `target/` cleaned between a
//! stopped merge and its commit, a rebase stopped and continued the same way,
//! `cargo` off the hook's `PATH`, and a launcher pinned to an engine that
//! ignores the flags and exits 0 having done nothing. Only an engine that has
//! regenerated and looked at the result removes the file.

mod common;

use std::fs;

use common::{Repo, write};

/// A merge that stops on a source conflict while the generated document also
/// differs, so the driver keeps ours for the document.
fn stopped_merge(r: &Repo) {
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(r.unmerged(), ["mock/ALPHA.md.tmpl"]);
    assert!(
        r.kept_ours().exists(),
        "the driver wrote down that it kept ours"
    );
}

fn resolve_template(r: &Repo) {
    write(
        &r.root.join("mock/ALPHA.md.tmpl"),
        "one\ntwo\nthree, resolved\nfour\nfive\n",
    );
    r.git(&["add", "mock/ALPHA.md.tmpl"]);
}

fn cargo_clean(r: &Repo) {
    fs::remove_dir_all(r.root.join("mock/target")).unwrap();
}

#[test]
fn a_commit_concluding_a_merge_is_blocked_when_target_was_cleaned_meanwhile() {
    let r = Repo::new();
    stopped_merge(&r);
    cargo_clean(&r);
    resolve_template(&r);

    let done = r.git_out(&["commit", "-q", "-m", "chore: merge a"]);
    let out = Repo::failure(&done);
    assert!(
        !done.status.success(),
        "the commit would carry one side's document: {out}"
    );
    assert!(out.contains("cargo mock"), "it names the fix: {out}");

    // the fix it names: generate, and the commit goes through regenerated
    r.generate();
    let again = r.git_out(&["commit", "-q", "-m", "chore: merge a"]);
    assert!(again.status.success(), "{}", Repo::failure(&again));
    let in_head = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(in_head.contains("three, resolved"), "{in_head}");
    assert!(
        !r.kept_ours().exists(),
        "the engine checked, so the record is gone"
    );
}

#[test]
fn a_rebase_continued_after_target_was_cleaned_does_not_finish_stale() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    r.git(&["switch", "-q", "a"]);
    let stopped = r.git_out(&["rebase", "dev"]);
    assert!(!stopped.status.success());
    assert!(r.kept_ours().exists());

    cargo_clean(&r);
    resolve_template(&r);
    let done = r
        .cmd("git")
        .env("GIT_EDITOR", "true")
        .args(["rebase", "--continue"])
        .output()
        .unwrap();
    let out = Repo::failure(&done);
    assert!(
        !done.status.success() || out.contains("may be stale"),
        "either the continue is refused or it says what it left: {out}"
    );
    assert!(r.kept_ours().exists() || !done.status.success(), "{out}");

    // whatever happened, nothing publishes it
    r.remote();
    let pushed = r.git_out(&["push", "origin", "a"]);
    if r.kept_ours().exists() {
        assert!(!pushed.status.success(), "{}", Repo::failure(&pushed));
        assert!(Repo::failure(&pushed).contains("cargo mock"));
    }
}

/// A merge with a hook whose `cargo` is missing, or an engine that does nothing.
fn merge_with(r: &Repo, env: &[(&str, &str)]) -> String {
    r.two_sides();
    let merged = r.git_env(env, &["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(
        r.kept_ours().exists(),
        "nothing regenerated, so the record stands"
    );
    let doc = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(
        !doc.contains("one, edited on a"),
        "the fixture left the document stale, as the case says: {doc}"
    );
    Repo::failure(&merged)
}

fn assert_push_refused_then_commit_heals(r: &Repo) {
    r.remote();
    let pushed = r.git_out(&["push", "origin", "dev"]);
    let out = Repo::failure(&pushed);
    assert!(
        !pushed.status.success(),
        "a stale document must not publish: {out}"
    );
    assert!(out.contains("cargo mock"), "{out}");

    // a commit with a working engine regenerates, stages and clears the record
    let healed = r.git_out(&["commit", "-q", "--allow-empty", "-m", "chore: after merge"]);
    assert!(healed.status.success(), "{}", Repo::failure(&healed));
    let doc = r.git(&["show", "HEAD:docs/ALPHA.md"]);
    assert!(doc.contains("one, edited on a"), "{doc}");
    assert!(doc.contains("five, edited on dev"), "{doc}");
    assert!(!r.kept_ours().exists());
}

#[test]
fn a_merge_whose_hook_cannot_find_cargo_says_so_and_nothing_publishes_it() {
    let r = Repo::new();
    let out = merge_with(&r, &[("MS_NO_CARGO", "1")]);
    assert!(out.contains("STALE"), "{out}");
    assert_push_refused_then_commit_heals(&r);
}

#[test]
fn a_merge_whose_engine_ignores_the_flags_says_so_and_nothing_publishes_it() {
    let r = Repo::new();
    let out = merge_with(&r, &[("MS_OLD_ENGINE", "1")]);
    assert!(out.contains("may be stale"), "{out}");
    assert_push_refused_then_commit_heals(&r);
}

#[test]
fn a_merge_that_was_aborted_leaves_nothing_to_refuse() {
    let r = Repo::new();
    stopped_merge(&r);
    r.git(&["merge", "--abort"]);
    // the record is left behind by the abort; with no hooks to regenerate with,
    // a commit that would carry nothing from the merge must not be refused for it
    cargo_clean(&r);
    let commit = r.git_out(&["commit", "-q", "--allow-empty", "-m", "chore: unrelated"]);
    assert!(commit.status.success(), "{}", Repo::failure(&commit));
    assert!(!r.kept_ours().exists());
}

#[test]
fn a_rebase_that_was_aborted_leaves_nothing_to_refuse() {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "three", "three, on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "three", "three, on dev");
    r.git(&["switch", "-q", "a"]);
    assert!(!r.git_out(&["rebase", "dev"]).status.success());
    assert!(
        r.kept_ours().exists(),
        "the driver wrote down that it kept ours"
    );
    r.git(&["rebase", "--abort"]);
    cargo_clean(&r);
    let commit = r.git_out(&["commit", "-q", "--allow-empty", "-m", "chore: unrelated"]);
    assert!(commit.status.success(), "{}", Repo::failure(&commit));
}

#[test]
fn an_ordinary_merge_leaves_no_record() {
    let r = Repo::new();
    r.two_sides();
    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(
        !r.kept_ours().exists(),
        "regenerated and checked, so nothing to refuse"
    );
}
