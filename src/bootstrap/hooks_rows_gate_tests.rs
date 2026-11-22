//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital

//! A commit of console-direct registry rows alone is gated by `mock rows-gate` and runs no
//! lint pass, and its push lints nothing the commit already passed on the same tree.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`; its stub answers a
//! `rows-gate` call with the number in `rows-gate.rc` beside its args file.

use std::fs;
use std::path::Path;

use super::hooks_src_dirs_tests::{KASKI, fixture, git, run_full, stage};
use super::*;

const ROW: (&str, &str) = (
    "mock/registry/ticket/items/a.toml",
    "[[ticket]]\nid = \"a\"\n",
);

fn rows_gate_answers(root: &Path, rc: u8) {
    fs::write(root.join("rows-gate.rc"), format!("{rc}\n")).unwrap();
}

fn pre_commit(root: &Path) -> (String, Vec<String>) {
    let hook = gen_pre_commit("mock", &HookLayout::of(KASKI), &root.join("no-user-hook"));
    run_full(&hook, root, "")
}

fn pre_push(root: &Path, base: &str, head: &str) -> (String, Vec<String>) {
    let hook = gen_pre_push("mock", &HookLayout::of(KASKI), &root.join("no-user-hook"));
    run_full(
        &hook,
        root,
        &format!("refs/heads/m {head} refs/heads/m {base}\n"),
    )
}

fn lint_passes(calls: &[String]) -> Vec<&String> {
    calls.iter().filter(|c| c.contains("--lint-only")).collect()
}

/// Record `commit`'s tree as one the rows gate passed, as `rows-gate` does.
fn record_gated(root: &Path, commit: &str) {
    let tree = git(root, &["rev-parse", &format!("{commit}^{{tree}}")]);
    let path = git(root, &["rev-parse", "--git-path", "mockspace/gated-rows"]);
    let path = root.join(path.trim());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let old = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, format!("{old}{}\n", tree.trim())).unwrap();
}

fn base_and_head(root: &Path, files: &[&[(&str, &str)]]) -> (String, Vec<String>) {
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]).trim().to_string();
    let mut heads = Vec::new();
    for f in files {
        stage(root, f);
        git(root, &["commit", "-q", "-m", "work"]);
        heads.push(git(root, &["rev-parse", "HEAD"]).trim().to_string());
    }
    (base, heads)
}

#[test]
fn a_ticket_only_commit_runs_the_rows_gate_and_no_lint_pass() {
    let tmp = fixture();
    let root = tmp.path();
    rows_gate_answers(root, 0);
    stage(root, &[ROW]);
    let (out, calls) = pre_commit(root);
    assert!(
        calls.iter().any(|c| c.contains("rows-gate")),
        "no rows-gate call: {calls:?}\n{out}"
    );
    assert!(
        lint_passes(&calls).is_empty(),
        "a lint pass ran: {calls:?}\n{out}"
    );
}

#[test]
fn rows_the_gate_does_not_take_run_the_gate_they_always_ran() {
    let tmp = fixture();
    let root = tmp.path();
    rows_gate_answers(root, 3);
    stage(root, &[ROW]);
    let (out, calls) = pre_commit(root);
    let passes = lint_passes(&calls);
    assert_eq!(passes.len(), 1, "{calls:?}\n{out}");
    assert!(passes[0].contains("--scope infra --doc-only"), "{passes:?}");
}

#[test]
fn a_red_rows_gate_blocks_the_commit_and_lints_nothing_more() {
    let tmp = fixture();
    let root = tmp.path();
    rows_gate_answers(root, 1);
    stage(root, &[ROW]);
    let (out, calls) = pre_commit(root);
    assert!(lint_passes(&calls).is_empty(), "{calls:?}\n{out}");
    let reported =
        out.contains("blocked: pre-commit") || calls.iter().any(|c| c.contains("gate-report"));
    assert!(reported, "red not reported: {calls:?}\n{out}");
}

#[test]
fn a_rows_gate_is_not_asked_when_anything_but_registry_rows_is_staged() {
    let tmp = fixture();
    let root = tmp.path();
    rows_gate_answers(root, 0);
    stage(root, &[ROW, ("mock/notes.md", "x\n")]);
    let (out, calls) = pre_commit(root);
    assert!(
        !calls.iter().any(|c| c.contains("rows-gate")),
        "{calls:?}\n{out}"
    );
    assert_eq!(lint_passes(&calls).len(), 1, "{calls:?}\n{out}");
}

#[test]
fn a_push_of_commits_the_rows_gate_passed_lints_nothing() {
    let tmp = fixture();
    let root = tmp.path();
    let second: (&str, &str) = (
        "mock/registry/ticket/items/b.toml",
        "[[ticket]]\nid = \"b\"\n",
    );
    let (base, heads) = base_and_head(root, &[&[ROW], &[second]]);
    for h in &heads {
        record_gated(root, h);
    }
    let (out, calls) = pre_push(root, &base, heads.last().unwrap());
    assert!(
        calls.iter().all(|c| !c.contains("--lint-only")),
        "a lint pass ran: {calls:?}\n{out}"
    );
}

#[test]
fn a_push_with_one_commit_the_rows_gate_did_not_pass_lints_as_before() {
    let tmp = fixture();
    let root = tmp.path();
    let second: (&str, &str) = (
        "mock/registry/ticket/items/b.toml",
        "[[ticket]]\nid = \"b\"\n",
    );
    let (base, heads) = base_and_head(root, &[&[ROW], &[second]]);
    record_gated(root, &heads[1]);
    let (out, calls) = pre_push(root, &base, &heads[1]);
    let passes = lint_passes(&calls);
    assert_eq!(passes.len(), 1, "{calls:?}\n{out}");
    assert!(passes[0].contains("--scope infra"), "{passes:?}");
}

#[test]
fn a_push_with_no_record_lints_as_before() {
    let tmp = fixture();
    let root = tmp.path();
    let (base, heads) = base_and_head(root, &[&[ROW]]);
    let (out, calls) = pre_push(root, &base, &heads[0]);
    assert_eq!(lint_passes(&calls).len(), 1, "{calls:?}\n{out}");
}

#[test]
fn a_recorded_tree_does_not_pass_a_push_that_publishes_a_new_branch() {
    let tmp = fixture();
    let root = tmp.path();
    let (_, heads) = base_and_head(root, &[&[ROW]]);
    record_gated(root, &heads[0]);
    let zeros = "0".repeat(heads[0].len());
    let (out, calls) = pre_push(root, &zeros, &heads[0]);
    assert_eq!(lint_passes(&calls).len(), 1, "{calls:?}\n{out}");
}
