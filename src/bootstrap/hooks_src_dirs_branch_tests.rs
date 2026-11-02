//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What each branch of the generated hooks passes to `cargo mock`, and the reason
//! the pre-push hook prints when it runs the full scope.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use super::hooks_src_dirs_tests::{
    CONSOLE_SRC,
    KASKI,
    commit_run,
    fixture,
    git,
    push_run,
    run_full,
    scope_call,
    stage,
};
use super::*;

/// The labels the hook echoes and the arguments it hands the launcher are two
/// things, and only the second decides what runs. Each branch is asserted on the
/// arguments the stub `cargo` recorded.
#[test]
fn the_commit_branches_pass_these_arguments() {
    let table: &[(&str, &[(&str, &str)], &str)] = &[
        (
            "one crate",
            &[(CONSOLE_SRC, "fn x() {}\n")],
            "mock --lint-only --commit --scope console-core",
        ),
        (
            "doc-only crate",
            &[(
                "mock/applets/console/crates/console-core/DESIGN.md.tmpl",
                "x\n",
            )],
            "mock --lint-only --commit --scope console-core --doc-only",
        ),
        (
            "no crate",
            &[("mock/notes.md", "x\n")],
            "mock --lint-only --commit --scope infra",
        ),
        (
            "registry row beside source: bounded to the crate, the repo lints read the row",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit --scope console-core",
        ),
        (
            "registry row alone: the repo lints and no crate's",
            &[("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit --scope infra --doc-only",
        ),
        (
            "registry rows alone, a ticket and its page under docs",
            &[("mock/registry/ticket/items/a.toml", "x = 1\n"), ("docs/TICKET-MOVE.md", "x\n")],
            "mock --lint-only --commit --scope infra --doc-only",
        ),
        (
            "registry row beside a note: the full scope a staged registry row always had, doc-only as no source is staged",
            &[("mock/registry/tenets.toml", "x = 1\n"), ("mock/notes.md", "x\n")],
            "mock --lint-only --commit --doc-only",
        ),
        (
            "config alone: every crate, the claim of no source kept",
            &[("mock/mockspace.toml", "x = 1\n")],
            "mock --lint-only --commit --doc-only",
        ),
        (
            "project lint beside a registry row: every crate, source lints on",
            &[("mock/lints/a.rs", "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit",
        ),
    ];
    for (what, files, want) in table {
        let (out, args) = commit_run(KASKI, files);
        assert_eq!(args, *want, "{what}: hook said:\n{out}");
    }
}

#[test]
fn the_push_branches_pass_these_arguments() {
    let table: &[(&str, &[(&str, &str)], &str)] = &[
        (
            "one crate",
            &[(CONSOLE_SRC, "fn x() {}\n")],
            "mock --lint-only --strict --scope console-core",
        ),
        (
            "no crate",
            &[("README.md", "x\n")],
            "mock --lint-only --strict",
        ),
        (
            "registry row beside source: bounded to the crate",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --strict --scope console-core",
        ),
        (
            "registry row alone: the repo lints and no crate's, and the claim the range changes no dependency input",
            &[("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --strict --scope infra --deps-unchanged",
        ),
        (
            "registry rows alone, a ticket and its page under docs",
            &[("mock/registry/ticket/items/a.toml", "x = 1\n"), ("docs/TICKET-MOVE.md", "x\n")],
            "mock --lint-only --strict --scope infra --deps-unchanged",
        ),
        (
            "registry row beside a file outside the mock directory that cargo-deny does not read: the claim stays",
            &[("mock/registry/ticket/items/a.toml", "id = \"a\"\n"), ("docs/notes.md", "x\n")],
            "mock --lint-only --strict --scope infra --deps-unchanged",
        ),
        (
            "registry row beside a note: the full run a push of anything else has",
            &[("mock/registry/tenets.toml", "x = 1\n"), ("mock/notes.md", "x\n")],
            "mock --lint-only --strict",
        ),
        // A dependency input outside the mock directory, which the diff the hook takes of the mock
        // directory does not see: each keeps the dependency gate with no claim made, and the full
        // scope, which `mock/benches/support/Cargo.lock` does not discriminate since it sits under
        // the mock directory and is caught as a file that is not a registry row either way.
        (
            "registry row beside the root manifest",
            &[("mock/registry/ticket/items/a.toml", "x = 1\n"), ("Cargo.toml", "x = 1\n")],
            "mock --lint-only --strict",
        ),
        (
            "registry row beside the root lockfile",
            &[("mock/registry/ticket/items/a.toml", "x = 1\n"), ("Cargo.lock", "x = 1\n")],
            "mock --lint-only --strict",
        ),
        (
            "registry row beside the deny config",
            &[("mock/registry/ticket/items/a.toml", "x = 1\n"), ("deny.toml", "x = 1\n")],
            "mock --lint-only --strict",
        ),
        (
            "registry row beside a nested manifest",
            &[
                ("mock/registry/ticket/items/a.toml", "x = 1\n"),
                ("tools/probe/Cargo.toml", "x = 1\n"),
            ],
            "mock --lint-only --strict",
        ),
        (
            "registry row beside cargo's config, where [source] and [patch] sit",
            &[
                ("mock/registry/ticket/items/a.toml", "x = 1\n"),
                (".cargo/config.toml", "x = 1\n"),
            ],
            "mock --lint-only --strict",
        ),
        (
            "project lint alone: every crate",
            &[("mock/lints/a.rs", "fn x() {}\n")],
            "mock --lint-only --strict",
        ),
        (
            "config alone: every crate",
            &[("mock/mockspace.toml", "x = 1\n")],
            "mock --lint-only --strict",
        ),
    ];
    for (what, files, want) in table {
        let (out, args) = push_run(KASKI, files);
        assert_eq!(args, *want, "{what}: hook said:\n{out}");
    }
}

/// A `mockspace.toml` a crate keeps as a test fixture is that crate's file, and
/// the config is named only where it lives: the repo root and the mock directory.
#[test]
fn a_fixture_named_like_the_config_stays_scoped_to_its_crate() {
    let fixture = "mock/applets/console/crates/console-core/tests/fixture/kaski/mockspace.toml";
    let (out, args) = commit_run(KASKI, &[(CONSOLE_SRC, "fn x() {}\n"), (fixture, "x = 1\n")]);
    assert_eq!(
        args, "mock --lint-only --commit --scope console-core",
        "{out}"
    );
    let (out, args) = push_run(KASKI, &[(CONSOLE_SRC, "fn x() {}\n"), (fixture, "x = 1\n")]);
    assert_eq!(
        args, "mock --lint-only --strict --scope console-core",
        "{out}"
    );
}

/// What the pre-push hook prints for one pushed ref whose remote side is
/// `remote(base)`, after a commit of `files` on top of a base commit.
fn push_with_remote(files: &[(&str, &str)], remote: impl Fn(&str) -> String) -> String {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, files);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {}\n", remote(&base));
    run_full(
        &gen_pre_push("mock", &HookLayout::of(KASKI), &root.join("no-user-hook")),
        root,
        &line,
    )
    .0
}

/// Each reason a push runs the full scope is printed as itself, and a shared
/// input on an existing branch is not called a new branch.
#[test]
fn the_full_scope_is_given_its_own_reason() {
    let zeros = "0".repeat(40);
    let cases: [(&str, Vec<(&str, &str)>, Box<dyn Fn(&str) -> String>, &str); 4] = [
        (
            "a new branch",
            vec![(CONSOLE_SRC, "fn x() {}\n")],
            Box::new(move |_| zeros.clone()),
            "(new branch)",
        ),
        (
            "a remote sha never fetched",
            vec![(CONSOLE_SRC, "fn x() {}\n")],
            Box::new(|_| "1".repeat(40)),
            "(remote sha not fetched)",
        ),
        (
            "a shared input on an existing branch",
            vec![("mock/lints/a.rs", "fn x() {}\n")],
            Box::new(|base| base.to_string()),
            "(shared input)",
        ),
        (
            "nothing under a source directory",
            vec![("docs/x.md", "x\n")],
            Box::new(|base| base.to_string()),
            "(no crate changes)",
        ),
    ];
    for (what, files, remote, reason) in cases {
        let out = push_with_remote(&files, remote);
        assert!(
            out.contains(&format!("scope: full project {reason}")),
            "{what}: hook said:\n{out}"
        );
    }
}

/// The arguments the pre-push hook hands the launcher for a push of two refs at
/// once: `first` is committed on the base and pushed as one ref, `second` on top
/// of that and pushed as the other, and `swap` puts the second ref's line first.
fn push_two_refs(first: &[(&str, &str)], second: &[(&str, &str)], swap: bool) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, first);
    git(root, &["commit", "-q", "-m", "first"]);
    let one = git(root, &["rev-parse", "HEAD"]);
    stage(root, second);
    git(root, &["commit", "-q", "-m", "second"]);
    let two = git(root, &["rev-parse", "HEAD"]);
    let mut lines = vec![
        format!("refs/heads/a {one} refs/heads/a {base}\n"),
        format!("refs/heads/b {two} refs/heads/b {one}\n"),
    ];
    if swap {
        lines.reverse();
    }
    let (printed, calls) = run_full(
        &gen_pre_push("mock", &HookLayout::of(KASKI), &root.join("no-user-hook")),
        root,
        &lines.concat(),
    );
    (printed, scope_call(&calls))
}

/// A dependency input in any one of the refs pushed together keeps the gate for
/// the whole push, in whichever order git lists them, and two refs of registry
/// rows alone are the repo-lints scope with its claim, as one is.
#[test]
fn a_dependency_input_in_either_of_two_refs_keeps_the_dependency_gate() {
    let row_a = ("mock/registry/ticket/items/a.toml", "x = 1\n");
    let row_b = ("mock/registry/ticket/items/b.toml", "x = 1\n");
    let manifest = ("Cargo.toml", "x = 1\n");
    for swap in [false, true] {
        let (out, args) = push_two_refs(&[row_a], &[manifest], swap);
        assert_eq!(
            args, "mock --lint-only --strict",
            "swap {swap}: hook said:\n{out}"
        );
        let (out, args) = push_two_refs(&[manifest], &[row_a], swap);
        assert_eq!(
            args, "mock --lint-only --strict",
            "swap {swap}: hook said:\n{out}"
        );
        let (out, args) = push_two_refs(&[row_a], &[row_b], swap);
        assert_eq!(
            args, "mock --lint-only --strict --scope infra --deps-unchanged",
            "swap {swap}, the control: hook said:\n{out}"
        );
    }
}
