//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// ── what counts as an input ──

#[test]
fn an_edited_arm_moves_the_digest() {
    assert!(moved_by(|_, m| {
        write(
            &m.join("benches/sample/arms/a/src/lib.rs"),
            "pub fn f() { 1; }\n",
        )
    }));
}

#[test]
fn an_added_support_file_moves_the_digest() {
    assert!(moved_by(|_, m| {
        write(&m.join("benches/support/src/lib.rs"), "")
    }));
}

#[test]
fn a_renamed_file_moves_the_digest() {
    assert!(moved_by(|_, m| {
        let d = m.join("benches/sample/arms/a/src");
        fs::rename(d.join("lib.rs"), d.join("other.rs")).unwrap();
    }));
}

#[test]
fn build_output_does_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/sample/arms/a/target/debug/junk"), "built");
    }));
}

/// `mock bench run` writes `results/` and `history/`, so counting them made
/// every bench run rerun every arm on the next `mock test`.
#[test]
fn generated_results_and_history_do_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/results/sample/a.csv"), "1,2");
        write(&m.join("benches/history/sample/a.toml"), "median = 1");
    }));
}

#[test]
fn an_untracked_file_that_git_ignores_does_not_move_the_digest() {
    assert!(!moved_by(|r, m| {
        write(&r.join(".gitignore"), "scratch/\n");
        write(&m.join("benches/scratch/x.rs"), "junk");
    }));
}

/// The toolchain pin, cargo's configuration, the manifest and the lockfile are
/// what the members' fingerprints are salted with, and an arm is built with
/// them too: every one of them, beside the mock directory and beside the
/// repository root.
#[test]
fn every_workspace_input_moves_the_digest_at_both_bases() {
    for name in WORKSPACE_INPUTS {
        for base in ["mock", "root"] {
            assert!(
                moved_by(|r, m| {
                    let b = if base == "root" { r } else { m };
                    write(&b.join(name), "changed");
                }),
                "{base}/{name} did not move the digest"
            );
        }
    }
}

/// kaski's support crates depend on workspace members by path. A broken member
/// has to run the benches again.
#[test]
fn a_path_dependency_of_a_bench_crate_moves_the_digest() {
    let setup = |r: &Path| {
        write(
            &r.join("mock/benches/support/s/Cargo.toml"),
            "[package]\nname = \"s\"\n[dependencies]\ndep = { path = \"../../../../crates/dep\" }\n",
        );
        write(
            &r.join("crates/dep/Cargo.toml"),
            "[package]\nname = \"dep\"\n",
        );
        write(&r.join("crates/dep/src/lib.rs"), "pub fn v() -> u8 { 1 }\n");
        write(
            &r.join("crates/other/src/lib.rs"),
            "pub fn v() -> u8 { 1 }\n",
        );
    };
    let (_t, root, mock) = repo();
    setup(&root);
    let before = now(&root, &mock);
    write(
        &root.join("crates/dep/src/lib.rs"),
        "pub fn v() -> u8 { 2 }\n",
    );
    assert_ne!(before, now(&root, &mock), "the dependency moved");
    let before = now(&root, &mock);
    write(
        &root.join("crates/other/src/lib.rs"),
        "pub fn v() -> u8 { 2 }\n",
    );
    assert_eq!(
        before,
        now(&root, &mock),
        "a crate nothing names is not an input"
    );
}

/// Cargo builds a bench crate's dev and build dependencies, and the same under
/// `[target.*]`, as it builds its plain ones.
#[test]
fn a_path_dependency_in_any_dependency_table_moves_the_digest() {
    let mut heads: Vec<String> = Vec::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        heads.push(kind.to_string());
        heads.push(format!("target.'cfg(unix)'.{kind}"));
        heads.push(format!("target.x86_64-unknown-linux-gnu.{kind}"));
    }
    for head in heads {
        let (_t, root, mock) = repo();
        write(
            &mock.join("benches/sample/Cargo.toml"),
            &format!("[{head}]\ndep = {{ path = \"../../../crates/dep\" }}\n"),
        );
        write(&root.join("crates/dep/src/lib.rs"), "1");
        let before = now(&root, &mock);
        write(&root.join("crates/dep/src/lib.rs"), "2");
        assert_ne!(before, now(&root, &mock), "[{head}]");
    }
}

#[test]
fn a_dependency_of_a_dependency_moves_the_digest() {
    let (_t, root, mock) = repo();
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\na = { path = \"../../../crates/a\" }\n",
    );
    write(
        &root.join("crates/a/Cargo.toml"),
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
    write(&root.join("crates/b/src/lib.rs"), "1");
    let before = now(&root, &mock);
    write(&root.join("crates/b/src/lib.rs"), "2");
    assert_ne!(before, now(&root, &mock));
}

#[test]
fn a_workspace_inherited_path_dependency_moves_the_digest() {
    let (_t, root, mock) = repo();
    write(
        &root.join("Cargo.toml"),
        "[workspace.dependencies]\ndep = { path = \"crates/dep\" }\n",
    );
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\ndep.workspace = true\n",
    );
    write(&root.join("crates/dep/src/lib.rs"), "1");
    let before = now(&root, &mock);
    write(&root.join("crates/dep/src/lib.rs"), "2");
    assert_ne!(before, now(&root, &mock));
}

#[test]
fn a_dependency_cycle_ends_and_still_reaches_every_member() {
    let (_t, root, mock) = repo();
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\na = { path = \"../../../crates/a\" }\n",
    );
    write(
        &root.join("crates/a/Cargo.toml"),
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
    write(
        &root.join("crates/b/Cargo.toml"),
        "[dependencies]\na = { path = \"../a\" }\n",
    );
    let before = now(&root, &mock);
    write(&root.join("crates/b/src/lib.rs"), "changed");
    assert_ne!(
        before,
        now(&root, &mock),
        "the far end of the cycle is an input"
    );
}

#[test]
fn a_directory_that_is_not_a_repository_has_no_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    write(&mock.join("benches/x.rs"), "");
    assert_eq!(digest(tmp.path(), &mock, &mock.join("benches")), None);
}
