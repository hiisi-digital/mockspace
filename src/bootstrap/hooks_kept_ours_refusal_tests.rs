//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The refusal both gating hooks print while a merge driver's kept-ours record stands: one
//! `blocked: <gate>` line and the one line `ms_kept_say` prints, and nothing else.
//!
//! The block is cut out of the generated hook and run, with the shared shell in front of it, in a
//! repository holding a merge in progress and the record, so what is pinned is what a person
//! reads rather than the shape of the script.

use std::path::Path;
use std::process::Command;

use mockspace_manifest::kept_ours::{KEPT_OURS_FILE, KEPT_OURS_SHELL};

use super::hooks::{HookLayout, gen_pre_commit, gen_pre_push};

/// The lines of the first `if ms_kept_live; then` block of a hook, up to its `fi`, trimmed.
fn refusal_block(hook: &str) -> Vec<String> {
    let mut lines = hook.lines().map(str::trim);
    assert!(
        lines.any(|l| l == "if ms_kept_live; then"),
        "control: the hook has a kept-ours refusal"
    );
    lines.take_while(|l| *l != "fi").map(str::to_string).collect()
}

fn refusal_run(block: &[String], repo: &Path) -> (i32, Vec<String>) {
    let script = format!("{KEPT_OURS_SHELL}\nif ms_kept_live; then\n{}\nfi\nexit 0\n", block.join("\n"));
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr).lines().map(str::to_string).collect();
    (out.status.code().unwrap(), err)
}

fn repo_with_a_kept_record() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(tmp.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    let dir = tmp.path().join(".git");
    std::fs::write(dir.join("MERGE_HEAD"), "0000000000000000000000000000000000000000\n").unwrap();
    std::fs::write(dir.join(KEPT_OURS_FILE), "HEAD-NOT-YET\nmock/generated/a.md\n").unwrap();
    tmp
}

#[test]
fn both_gating_hooks_refuse_a_kept_record_in_one_blocked_line_and_the_one_say_line() {
    let pre_commit = gen_pre_commit("mock", &HookLayout::of(&["crates"]), Path::new("none"));
    let pre_push = gen_pre_push("mock", &HookLayout::of(&["crates"]), Path::new("none"));
    for (gate, hook) in [("pre-commit", pre_commit), ("pre-push", pre_push)] {
        let tmp = repo_with_a_kept_record();
        let block = refusal_block(&hook);
        let (code, err) = refusal_run(&block, tmp.path());
        assert_eq!(code, 1, "{gate}: the record refuses");
        assert_eq!(err.len(), 2, "{gate}: one blocked line and one say line, got {err:?}");
        assert_eq!(err[0], format!("blocked: {gate}"));
        assert!(
            err[1].starts_with("mockspace: generated files kept from one side")
                && err[1].contains("mock/generated/a.md"),
            "{gate}: the say line names the kept paths, got {:?}",
            err[1]
        );
    }
}

#[test]
fn the_refusal_run_prints_nothing_and_passes_when_no_record_stands() {
    let tmp = repo_with_a_kept_record();
    std::fs::remove_file(tmp.path().join(".git").join(KEPT_OURS_FILE)).unwrap();
    let hook = gen_pre_push("mock", &HookLayout::of(&["crates"]), Path::new("none"));
    let (code, err) = refusal_run(&refusal_block(&hook), tmp.path());
    assert_eq!((code, err), (0, Vec::new()), "control: the same block passes silently without the record");
}
