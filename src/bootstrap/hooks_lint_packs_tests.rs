//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated hooks treat a project lint and a lint pack declared by path as
//! shared inputs, and the layout reads the packs from the config.
//!
//! The fixture and the hook runner are those of `hooks_src_dirs_tests`.

use std::fs;

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

/// A project lint is compiled into the run, so a commit that edits one and no
/// crate must not be scoped to `infra`, where the changed lint never runs.
#[test]
fn a_project_lint_is_a_shared_input() {
    let (out, args) = commit_run(KASKI, &[("mock/lints/no-foo/src/lib.rs", "fn l() {}\n")]);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    let (out, args) = push_run(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        ("mock/lints/no-foo/src/lib.rs", "fn l() {}\n"),
    ]);
    assert_eq!(args, "mock --lint-only --strict", "{out}");
}

const PACK: &str = "mock/crates/kaski-asset-lints";

fn commit_with_packs(
    packs: &[&str],
    base: &[(&str, &str)],
    files: &[(&str, &str)],
) -> (String, String) {
    let tmp = fixture();
    if !base.is_empty() {
        stage(tmp.path(), base);
        git(tmp.path(), &["commit", "-q", "-m", "base"]);
    }
    stage(tmp.path(), files);
    let (printed, calls) = run_full(
        &gen_pre_commit(
            "mock",
            &HookLayout::of_with_packs(KASKI, packs),
            &tmp.path().join("no-user-hook"),
        ),
        tmp.path(),
        "",
    );
    (printed, scope_call(&calls))
}

fn push_with_packs(packs: &[&str], files: &[(&str, &str)]) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, files);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {base}\n");
    let (printed, calls) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of_with_packs(KASKI, packs),
            &root.join("no-user-hook"),
        ),
        root,
        &line,
    );
    (printed, scope_call(&calls))
}

/// A lint pack declared by path is code the run compiles, as a file under the mock
/// directory's `lints/` is. One that sits under a source directory is also a crate,
/// and the crate reading would bound the commit to it, so the changed lint would
/// never run over the others.
#[test]
fn a_pack_under_a_source_directory_is_a_shared_input() {
    let src = format!("{PACK}/src/lib.rs");
    let manifest = format!("{PACK}/Cargo.toml");
    for file in [&src, &manifest] {
        let (out, args) = commit_with_packs(&[PACK], &[], &[(file, "x\n")]);
        assert_eq!(args, "mock --lint-only --commit", "{file}: {out}");
        let (out, args) = push_with_packs(&[PACK], &[(file, "x\n")]);
        assert_eq!(args, "mock --lint-only --strict", "{file}: {out}");
    }
}

/// The control: a sibling crate in the same source directory stays bounded to
/// itself, so the arm above is about the pack and not about the directory.
#[test]
fn a_crate_beside_a_pack_stays_scoped_to_itself() {
    let sibling = "mock/crates/kaski-sim/src/lib.rs";
    let (out, args) = commit_with_packs(&[PACK], &[], &[(sibling, "fn s() {}\n")]);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim", "{out}");
    let (out, args) = push_with_packs(&[PACK], &[(sibling, "fn s() {}\n")]);
    assert_eq!(args, "mock --lint-only --strict --scope kaski-sim", "{out}");
}

/// A pack that is declared but has not been staged leaves a commit as before.
#[test]
fn the_same_edit_without_a_declared_pack_is_scoped_to_its_crate() {
    let src = format!("{PACK}/src/lib.rs");
    let (out, args) = commit_with_packs(&[], &[], &[(&src, "fn l() {}\n")]);
    assert_eq!(
        args, "mock --lint-only --commit --scope kaski-asset-lints",
        "{out}"
    );
}

/// A pack outside the mock directory is staged outside the surface the hook looks
/// at, and still enters the gate with the full scope.
#[test]
fn a_pack_outside_the_mock_directory_enters_the_gate() {
    let (out, args) = commit_with_packs(&["tools/packs/p"], &[], &[(
        "tools/packs/p/src/lib.rs",
        "fn l() {}\n",
    )]);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    let (out, args) = push_with_packs(&["tools/packs/p"], &[(
        "tools/packs/p/src/lib.rs",
        "fn l() {}\n",
    )]);
    assert_eq!(args, "mock --lint-only --strict", "{out}");
}

/// The packs the layout carries are those the config declares by path, resolved the
/// way cargo reads them: from the generated lint crate at
/// `<mock>/target/mockspace-lints/`, and kept as repository-relative directories.
#[test]
fn the_layout_reads_the_path_packs_of_the_config() {
    let tmp = fixture();
    let root = tmp.path();
    let mock = root.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let config = root.join("mockspace.toml");
    fs::write(
        &config,
        "[lint-crates]\n\
         remote = { git = \"ssh://git@github.com/o/r.git\", branch = \"dev\" }\n\
         version = \"0.1\"\n\
         inline = { path = \"../../crates/kaski-asset-lints\" }\n\
         outside = { path = \"../../../../elsewhere\" }\n\
         [lint-crates.table]\n\
         path = \"../../../tools/table-pack/\"\n",
    )
    .unwrap();
    let layout = HookLayout::declared(root, &mock, &[mock.join("crates")], "m", &config);
    assert_eq!(layout.lint_packs, vec![
        "mock/crates/kaski-asset-lints".to_string(),
        "tools/table-pack".to_string(),
    ]);
    // no config, no packs
    let none = HookLayout::declared(root, &mock, &[], "m", &root.join("absent.toml"));
    assert!(none.lint_packs.is_empty());
}
