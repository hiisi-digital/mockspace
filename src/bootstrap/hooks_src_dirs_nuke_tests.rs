//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated pre-push hook and the engine agree on when a workspace is nuked.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use std::fs;

use super::hooks_src_dirs_tests::{KASKI, fixture, git, push_run, run_full, stage};
use super::*;

const NUKED: &str = "// Nuked by `cargo mock --nuke`\n";

/// Whether the push hook and the engine each call the workspace nuked, over the
/// same tree: `lib_rs` is every crate's `src/lib.rs` body, by path.
fn nuke_verdicts(lib_rs: &[(&str, &str)]) -> (bool, bool) {
    let (out, _) = push_run(KASKI, lib_rs);
    let hook = out.contains("nuked workspace");

    let tmp = fixture();
    for (path, body) in lib_rs {
        let p = tmp.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
    }
    let mock = tmp.path().join("mock");
    let mut cfg = crate::config::Config::from_dir(&mock);
    cfg.src_dirs = vec![mock.join("crates"), mock.join("applets/console/crates")];
    let engine = crate::entry::detect_nuked_workspace(&cfg);
    (hook, engine)
}

const ENGINE_LIB: &str = "mock/crates/kaski-sim/src/lib.rs";
const CONSOLE_LIB: &str = "mock/applets/console/crates/console-core/src/lib.rs";

#[test]
fn a_workspace_with_every_crate_nuked_is_nuked_for_the_hook_and_the_engine() {
    let (hook, engine) = nuke_verdicts(&[(ENGINE_LIB, NUKED), (CONSOLE_LIB, NUKED)]);
    assert!(engine, "the engine calls it nuked");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

/// One nuked console crate beside a live engine crate is not a nuked workspace.
/// The hook took any crate for all, and turned the source lints off for the live
/// engine crate.
#[test]
fn one_nuked_crate_beside_a_live_one_is_not_nuked_for_the_hook_or_the_engine() {
    let (hook, engine) = nuke_verdicts(&[(ENGINE_LIB, "fn s() {}\n"), (CONSOLE_LIB, NUKED)]);
    assert!(!engine, "the engine calls it live");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

#[test]
fn the_nuke_marker_is_the_configured_one() {
    let root = fixture();
    stage(root.path(), &[
        ("README", "base\n"),
        (CONSOLE_LIB, "// Nuked by a design round\n"),
    ]);
    git(root.path(), &["commit", "-q", "-m", "base"]);
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {base} refs/heads/m {}\n", "1".repeat(40));
    let layout = HookLayout {
        nuke_marker: "Nuked by a design round".to_string(),
        ..HookLayout::of(&["applets/console/crates"])
    };
    let (out, _) = run_full(
        &gen_pre_push("mock", &layout, &root.path().join("no-user-hook")),
        root.path(),
        &line,
    );
    assert!(out.contains("nuked workspace"), "hook said:\n{out}");
    // the same tree under the default marker is live
    let (out, _) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of(&["applets/console/crates"]),
            &root.path().join("no-user-hook"),
        ),
        root.path(),
        &line,
    );
    assert!(!out.contains("nuked workspace"), "hook said:\n{out}");
}

/// As `nuke_verdicts`, over one tree that `setup` builds on disk, so it can hold
/// what a list of files cannot: a dot-directory, a symlink.
fn verdicts_over_tree(setup: &dyn Fn(&std::path::Path)) -> (bool, bool) {
    let tmp = fixture();
    let root = tmp.path();
    setup(root);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, &[("README", "work\n")]);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {base}\n");
    let (out, _) = run_full(
        &gen_pre_push("mock", &HookLayout::of(KASKI), &root.join("no-user-hook")),
        root,
        &line,
    );
    let mock = root.join("mock");
    let mut cfg = crate::config::Config::from_dir(&mock);
    cfg.src_dirs = vec![mock.join("crates"), mock.join("applets/console/crates")];
    (
        out.contains("nuked workspace"),
        crate::entry::detect_nuked_workspace(&cfg),
    )
}

fn write_lib(root: &std::path::Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// The engine lists a dot-directory as a package, so a live crate in one makes the
/// workspace live. The hook's glob skipped it and called the workspace nuked, which
/// turned the source lints off over live source.
#[test]
fn a_live_crate_in_a_dot_directory_is_live_for_the_hook_and_the_engine() {
    let (hook, engine) = verdicts_over_tree(&|root| {
        write_lib(root, ENGINE_LIB, NUKED);
        write_lib(root, "mock/crates/.hidden/src/lib.rs", "fn live() {}\n");
    });
    assert!(!engine, "the engine counts the dot-directory as live");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

/// The control: a nuked crate in a dot-directory is counted too, so the arm above
/// is about the dot-directory being listed and not about it being live.
#[test]
fn a_nuked_crate_in_a_dot_directory_beside_nuked_ones_is_nuked() {
    let (hook, engine) = verdicts_over_tree(&|root| {
        write_lib(root, ENGINE_LIB, NUKED);
        write_lib(root, "mock/crates/.hidden/src/lib.rs", NUKED);
    });
    assert!(engine, "every crate is nuked");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

/// The engine does not follow a symlinked directory, so a link to a live crate is
/// not a package. The hook's glob followed it and called the workspace live.
#[test]
fn a_symlinked_crate_directory_is_not_a_crate_for_the_hook_or_the_engine() {
    let (hook, engine) = verdicts_over_tree(&|root| {
        write_lib(root, ENGINE_LIB, NUKED);
        write_lib(root, "mock/elsewhere/src/lib.rs", "fn live() {}\n");
        std::os::unix::fs::symlink("../elsewhere", root.join("mock/crates/linked")).unwrap();
    });
    assert!(engine, "the engine skips the link");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}
