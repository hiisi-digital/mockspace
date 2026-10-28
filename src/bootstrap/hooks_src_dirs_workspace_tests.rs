//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated hooks scope a manifest or a lockfile to the crates of its own
//! workspace, on the kaski shape of a mock workspace beside a console workspace.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use super::hooks::{HookLayout, gen_pre_commit};
use super::hooks_src_dirs_tests::{
    KASKI, commit_run_on, fixture, git, push_run_on, run_full, scope_call, stage,
};

/// The kaski shape on disk: the mock workspace and the console workspace, each
/// with its manifest and lockfile and its own crates.
pub(super) const KASKI_TREE: &[(&str, &str)] = &[
    ("mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
    ("mock/Cargo.lock", "# lock 1\n"),
    ("mock/crates/kaski-sim/src/lib.rs", "fn s() {}\n"),
    ("mock/crates/kaski-ui/src/lib.rs", "fn u() {}\n"),
    (
        "mock/applets/console/Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    ),
    ("mock/applets/console/Cargo.lock", "# lock 1\n"),
    (
        "mock/applets/console/crates/console-core/src/lib.rs",
        "fn c() {}\n",
    ),
    (
        "mock/applets/console/crates/console-serve/src/lib.rs",
        "fn v() {}\n",
    ),
    (
        "mock/applets/console/crates/console-app/src/lib.rs",
        "fn a() {}\n",
    ),
];

const CONSOLE_SCOPE: &str = "console-app,console-core,console-serve";

#[test]
fn the_console_lockfile_alone_scopes_to_the_console_crates() {
    let staged = [("mock/applets/console/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --strict --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn the_console_manifest_alone_scopes_to_the_console_crates() {
    let staged = [(
        "mock/applets/console/Cargo.toml",
        "[workspace]\nmembers = []\n",
    )];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn console_source_and_its_lockfile_scope_to_all_three_crates() {
    let staged = [
        ("mock/applets/console/Cargo.lock", "# lock 2\n"),
        (
            "mock/applets/console/crates/console-core/src/x.rs",
            "fn x() {}\n",
        ),
    ];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --strict --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn the_mock_lockfile_scopes_to_the_mock_workspace_and_not_the_console() {
    let staged = [("mock/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args, "mock --lint-only --commit --scope kaski-sim,kaski-ui",
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args, "mock --lint-only --strict --scope kaski-sim,kaski-ui",
        "{out}"
    );
}

/// The control for the arms above: a crate's own lockfile is not a workspace root,
/// so it moves no other crate. It is that crate's source, since a changed
/// dependency can change what that crate's lints see, so the scope keeps the source
/// lints on.
#[test]
fn a_crates_own_lockfile_is_that_crates_source_and_moves_no_workspace() {
    let staged = [("mock/crates/kaski-sim/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim", "{out}");
}

/// A `[features]` or `[lints]` edit lives in the crate's own manifest, and a
/// `--doc-only` run skips the very source lints that edit changes.
#[test]
fn a_crates_own_manifest_is_that_crates_source_not_doc_only() {
    let staged = [(
        "mock/applets/console/crates/console-core/Cargo.toml",
        "[package]\nname = \"console-core\"\n[features]\nx = []\n",
    )];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit --scope console-core", "{out}");
}

/// The control: a documentation file of the same crate is still doc-only, so the
/// two arms above are about the manifest and not about the flag being gone.
#[test]
fn a_crates_documentation_alone_is_still_doc_only() {
    let docs = [("mock/crates/kaski-sim/DESIGN.md.tmpl", "x\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &docs);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim --doc-only", "{out}");
}

/// A shared input staged beside a workspace lockfile keeps `--doc-only` off: the
/// lockfile is a changed dependency, which can change what the crates' lints
/// report. Without the guard on the workspace crates the shared-input arm adds
/// the flag back.
#[test]
fn a_shared_input_beside_a_workspace_lockfile_is_not_doc_only() {
    let staged = [
        ("mock/registry/tenets.toml", "x = 1\n"),
        ("mock/Cargo.lock", "# lock 2\n"),
    ];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    // and the shared input alone, over the same tree, is doc-only
    let alone = [("mock/registry/tenets.toml", "x = 1\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &alone);
    assert_eq!(args, "mock --lint-only --commit --doc-only", "{out}");
}

/// What the pre-commit hook scopes a staged mock lockfile to, over a tree built on
/// disk by `setup` so it can hold a dot-directory and a symlink.
fn lockfile_scope_over(setup: &dyn Fn(&std::path::Path)) -> String {
    let tmp = fixture();
    let root = tmp.path();
    setup(root);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    stage(root, &[("mock/Cargo.lock", "# lock 2\n")]);
    let (out, calls) = run_full(
        &gen_pre_commit("mock", &HookLayout::of(&["crates"]), &root.join("no-user-hook")),
        root,
        "",
    );
    let call = scope_call(&calls);
    call.split("--scope ")
        .nth(1)
        .unwrap_or_else(|| panic!("no scope in `{call}`:\n{out}"))
        .trim()
        .to_string()
}

fn put(root: &std::path::Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// The engine lists a dot-directory as a package, so a lockfile moves it too. The
/// hook's plain glob left it out of the scope.
#[test]
fn a_lockfile_scopes_to_a_crate_in_a_dot_directory() {
    let scope = lockfile_scope_over(&|root| {
        put(root, "mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        put(root, "mock/Cargo.lock", "# lock 1\n");
        put(root, "mock/crates/real/src/lib.rs", "fn r() {}\n");
        put(root, "mock/crates/.hidden/src/lib.rs", "fn h() {}\n");
    });
    assert_eq!(scope, ".hidden,real");
}

/// The engine does not follow a symlinked directory, so a link is not a crate and
/// a lockfile moves nothing through it. The hook's glob followed it.
#[test]
fn a_lockfile_does_not_scope_to_a_symlinked_directory() {
    let scope = lockfile_scope_over(&|root| {
        put(root, "mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        put(root, "mock/Cargo.lock", "# lock 1\n");
        put(root, "mock/crates/real/src/lib.rs", "fn r() {}\n");
        put(root, "mock/elsewhere/src/lib.rs", "fn e() {}\n");
        std::os::unix::fs::symlink("../elsewhere", root.join("mock/crates/linked")).unwrap();
    });
    assert_eq!(scope, "real");
}
