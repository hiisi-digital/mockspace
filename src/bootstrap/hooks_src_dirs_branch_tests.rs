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
            "shared input beside source",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit",
        ),
        (
            "shared input alone",
            &[("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit --doc-only",
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
            "shared input beside source",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
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
            vec![("mock/registry/tenets.toml", "[[tenet]]\nid = \"a\"\n")],
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
