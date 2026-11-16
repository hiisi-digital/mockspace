//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit and push gates refuse a subject the project's convention does not allow, with no
//! lint pack imported at all.
//!
//! `bench:` commits passed the gate because a project that imported nothing had no message lint,
//! and a gate with no lint returned success. These drive the real entry the hooks call, with an
//! empty pack, so the gate is held to refusing on its own.

use std::process::ExitCode;

use mockspace_lint_rules::{LintMode, LintPack, MessageDomain};

use super::message::{Request, run, run_batch};
use crate::config::Config;

fn project(lints: &str) -> (tempfile::TempDir, Config) {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    std::fs::create_dir_all(mock.join("agent")).unwrap();
    std::fs::write(
        tmp.path().join("mockspace.toml"),
        format!(
            "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\nmock_dir = \"mock\"\n{lints}"
        ),
    )
    .unwrap();
    let cfg = Config::from_dir(&mock);
    (tmp, cfg)
}

fn commit(cfg: &Config, gate: LintMode, subject: &str) -> ExitCode {
    let req = Request {
        domain:    MessageDomain::CommitMessage,
        message:   format!("{subject}\n"),
        origin:    "COMMIT_EDITMSG".to_string(),
        command:   None,
        tool:      None,
        author:    None,
        committer: None,
    };
    run(cfg, &LintPack::default(), gate, &req)
}

#[test]
fn an_unlisted_type_is_refused_at_the_commit_gate_with_no_pack() {
    // Mutant: `run` returns success when the pack has no message lints.
    let (_tmp, cfg) = project("");
    assert_eq!(
        commit(&cfg, LintMode::Commit, "research: sketch a lint"),
        ExitCode::FAILURE
    );
    assert_eq!(
        commit(&cfg, LintMode::Commit, "bench: land the results"),
        ExitCode::SUCCESS
    );
}

#[test]
fn every_gate_refuses_it_and_not_only_the_commit_one() {
    // Mutant: the lint's default severity is weaker at build or push.
    let (_tmp, cfg) = project("");
    for gate in [LintMode::Commit, LintMode::Build, LintMode::Push] {
        assert_eq!(
            commit(&cfg, gate, "Update the thing."),
            ExitCode::FAILURE,
            "{gate:?}"
        );
    }
}

#[test]
fn a_type_the_project_adds_is_accepted_and_the_rest_still_are_not() {
    // Mutant: `extra_types` is not carried from `mockspace.toml` to the lint.
    let (_tmp, cfg) = project("[lints.commit-subject]\nextra_types = \"research\"\n");
    assert_eq!(
        commit(&cfg, LintMode::Commit, "research: sketch a lint"),
        ExitCode::SUCCESS
    );
    assert_eq!(
        commit(&cfg, LintMode::Commit, "perf: tune it"),
        ExitCode::FAILURE
    );
}

#[test]
fn a_project_that_wants_no_convention_turns_it_off_with_the_severity() {
    // Mutant: the builtin cannot be switched off.
    let (_tmp, cfg) =
        project("[lints.commit-subject]\ncommit = \"off\"\nbuild = \"off\"\npush = \"off\"\n");
    assert_eq!(
        commit(&cfg, LintMode::Commit, "anything goes"),
        ExitCode::SUCCESS
    );
}

#[test]
fn the_push_gate_judges_every_commit_in_the_batch() {
    // Mutant: the batch path skips the builtin the single path runs.
    let (_tmp, cfg) = project("");
    let batch = "A <a@x>\x1eA <a@x>\x1eaaa1111 bench: ok\x1fbench: ok\n\0\
                 A <a@x>\x1eA <a@x>\x1ebbb2222 Wip\x1fWip\n\0\
                 A <a@x>\x1eA <a@x>\x1eccc3333 feat: fine\x1ffeat: fine\n\0";
    let (checked, failed) = run_batch(
        &cfg,
        &LintPack::default(),
        LintMode::Push,
        MessageDomain::CommitMessage,
        batch,
        "<stdin>",
        None,
        None,
    );
    assert_eq!((checked, failed), (3, 1));
}

#[test]
fn commit_off_alone_turns_every_gate_off_the_push_gate_included() {
    // Mutant: `commit = "off"` leaves build and push on, so the form the docs describe still
    // refuses at the push. The other off-test sets all three keys and cannot see this.
    let (_tmp, cfg) = project("[lints.commit-subject]\ncommit = \"off\"\n");
    for gate in [LintMode::Commit, LintMode::Build, LintMode::Push] {
        assert_eq!(
            commit(&cfg, gate, "anything goes"),
            ExitCode::SUCCESS,
            "{gate:?}"
        );
    }
    let batch = "A <a@x>\x1eA <a@x>\x1eaaa1111 Wip\x1fWip\n\0";
    let (checked, failed) = run_batch(
        &cfg,
        &LintPack::default(),
        LintMode::Push,
        MessageDomain::CommitMessage,
        batch,
        "<stdin>",
        None,
        None,
    );
    assert_eq!((checked, failed), (1, 0));
}

#[test]
fn an_autosquash_commit_passes_the_commit_gate_and_is_refused_by_the_push_gate() {
    // Mutant: `fixup!` is exempt at every gate, so it reaches the trunk unfolded.
    let (_tmp, cfg) = project("");
    assert_eq!(
        commit(&cfg, LintMode::Commit, "fixup! fix: handle it"),
        ExitCode::SUCCESS
    );
    assert_eq!(
        commit(&cfg, LintMode::Push, "fixup! fix: handle it"),
        ExitCode::FAILURE
    );
    let batch = "A <a@x>\x1eA <a@x>\x1eaaa1111 fixup! fix: x\x1ffixup! fix: x\n\0";
    let (_, failed) = run_batch(
        &cfg,
        &LintPack::default(),
        LintMode::Push,
        MessageDomain::CommitMessage,
        batch,
        "<stdin>",
        None,
        None,
    );
    assert_eq!(failed, 1);
}
