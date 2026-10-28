//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated hooks scope a manifest or a lockfile to the crates of its own
//! workspace, on the kaski shape of a mock workspace beside a console workspace.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use super::hooks_src_dirs_tests::{KASKI, commit_run_on, push_run_on};

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
