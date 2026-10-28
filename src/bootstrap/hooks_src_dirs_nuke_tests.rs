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
