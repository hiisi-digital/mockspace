//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The dependency gate reads `--deps-unchanged` at the one place the binary
//! decides, on the argument vector a pre-push hook sends.
//!
//! The decision itself is a pure function with its own unit tests, and those
//! pass whatever the call site hands it: a call that passed no arguments, or a
//! hard-coded mode, leaves them green and the gate never stands down. This runs
//! the real binary instead, in the scratch repository the other end-to-end tests
//! use, with a `deny.toml` at its root and a `cargo` that has no `deny` command,
//! so a gate that runs says so by naming the missing tool, and a gate that
//! stands down says so by naming the flag.
//!
//! Four argument vectors:
//! - the hook's: a push with the repo-lints scope and the claim stands down;
//! - the same scope without the claim runs the gate, which is a crate named
//!   `infra` and every hook generated before the flag;
//! - a plain strict push runs it;
//! - a commit with the claim does not reach it at all, since the gate is a push
//!   gate.

use std::process::Output;

#[path = "common/ambient.rs"]
mod ambient;
mod common;
use common::{BIN, Repo, write};

const STOOD_DOWN: &str = "skipped, --deps-unchanged";
const RAN: &str = "cargo-deny not installed";

fn fixture() -> Repo {
    let repo = Repo::new();
    write(&repo.root.join("deny.toml"), "[graph]\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "--no-verify", "-m", "chore: add a deny config"]);
    repo
}

fn run(repo: &Repo, args: &[&str]) -> (String, Output) {
    let out = repo
        .cmd(BIN)
        .current_dir(repo.root.join("mock"))
        .args(args)
        .output()
        .expect("the binary runs");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    (said, out)
}

#[test]
fn the_hooks_argument_vector_stands_the_gate_down() {
    let repo = fixture();
    let (said, _) = run(&repo, &[
        "--lint-only",
        "--strict",
        "--scope",
        "infra",
        "--deps-unchanged",
    ]);
    assert!(
        said.contains(STOOD_DOWN),
        "the gate did not stand down:\n{said}"
    );
    assert!(!said.contains(RAN), "the gate ran as well:\n{said}");
}

#[test]
fn the_repo_lints_scope_without_the_claim_runs_the_gate() {
    // The control for the arm above: the same fixture and scope, so a binary that
    // never reached the gate at all would leave this arm without the line it
    // looks for, and the arm above without the line it looks for.
    let repo = fixture();
    let (said, _) = run(&repo, &["--lint-only", "--strict", "--scope", "infra"]);
    assert!(said.contains(RAN), "the gate did not run:\n{said}");
    assert!(
        !said.contains(STOOD_DOWN),
        "the gate stood down without a claim:\n{said}"
    );
}

#[test]
fn a_plain_strict_push_runs_the_gate() {
    let repo = fixture();
    let (said, _) = run(&repo, &["--lint-only", "--strict"]);
    assert!(said.contains(RAN), "the gate did not run:\n{said}");
    assert!(
        !said.contains(STOOD_DOWN),
        "the gate stood down without a claim:\n{said}"
    );
}

#[test]
fn a_commit_with_the_claim_never_reaches_the_gate() {
    let repo = fixture();
    let (said, _) = run(&repo, &[
        "--lint-only",
        "--commit",
        "--scope",
        "infra",
        "--deps-unchanged",
    ]);
    assert!(
        !said.contains(STOOD_DOWN),
        "a commit ran the push gate:\n{said}"
    );
    assert!(!said.contains(RAN), "a commit ran the push gate:\n{said}");
}
