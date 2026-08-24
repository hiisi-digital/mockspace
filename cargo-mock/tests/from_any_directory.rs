//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The launcher answers the same from every working directory in a repo.
//!
//! This is the property the launcher exists for, and it is the one no unit test
//! can establish: `renki` resolves from the process's own current directory, so
//! only running the built binary with a working directory actually set proves
//! anything about it.
//!
//! Both installed spellings are exercised, because they are two binaries and
//! "they call the same library" is a claim about the source rather than about
//! what got installed.

use std::path::Path;
use std::process::Command;

/// A repo with a root config mapping a `mock` workspace, and a few directories
/// deep inside it to run from.
fn witness(dir: &Path) {
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(
        dir.join("mockspace.toml"),
        "project_name = \"witness\"\nmockspace_branch = \"dev\"\nmock_dir = \"mock\"\n",
    )
    .unwrap();
    for sub in ["mock/crates/thing/src", "mock/design_rounds", "src", ".github"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
}

fn locate_from(bin: &str, cwd: &Path) -> String {
    let out = Command::new(env!(concat!("CARGO_BIN_EXE_", "mock")))
        .arg("locate")
        .current_dir(cwd)
        .env_remove("MOCK_ROOT")
        .output()
        .unwrap_or_else(|e| panic!("running {bin}: {e}"));
    assert!(
        out.status.success(),
        "{bin} in {}: {}",
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn locate_answers_identically_from_every_directory_in_the_repo() {
    let d = tempfile::tempdir().unwrap();
    // canonicalised, because macOS hands out a `/var` tempdir that resolves
    // through `/private/var`, and the launcher reports the resolved form.
    let root = d.path().canonicalize().unwrap();
    witness(&root);

    let want = format!(
        "root={}\nconfig={}\nworkdir={}\n",
        root.display(),
        root.join("mockspace.toml").display(),
        root.join("mock").display()
    );

    for from in [
        ".",
        "mock",
        "mock/crates/thing/src",
        "mock/design_rounds",
        "src",
        ".github",
    ] {
        assert_eq!(
            locate_from("mock", &root.join(from)),
            want,
            "answered differently from `{from}`"
        );
    }
}

#[test]
fn the_walk_stops_at_the_nearest_repo_and_not_at_an_outer_one() {
    // the control that gives the test above its meaning: sameness everywhere
    // would also be satisfied by a launcher that always answered the outermost
    // repo, or that ignored the working directory entirely.
    let d = tempfile::tempdir().unwrap();
    let outer = d.path().canonicalize().unwrap();
    witness(&outer);
    let inner = outer.join("member");
    std::fs::create_dir_all(&inner).unwrap();
    witness(&inner);

    let from_inner = locate_from("mock", &inner.join("mock"));
    assert!(
        from_inner.contains(&format!("root={}\n", inner.display())),
        "the walk did not stop at the member repo:\n{from_inner}"
    );
    assert!(
        !from_inner.contains(&format!("root={}\n", outer.display())),
        "the walk reached past the member repo:\n{from_inner}"
    );
}

#[test]
fn both_installed_binaries_answer_the_same_thing() {
    // `cargo-mock` is cargo's external-subcommand spelling and is invoked as
    // `cargo-mock mock <args>`, so it must drop the repeated subcommand name.
    // A launcher that did not would pass `mock` to the engine as an argument.
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    witness(&root);

    let direct = locate_from("mock", &root.join("mock"));

    let out = Command::new(env!("CARGO_BIN_EXE_cargo-mock"))
        .args(["mock", "locate"])
        .current_dir(root.join("mock"))
        .env_remove("MOCK_ROOT")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cargo-mock: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), direct);
}

#[test]
fn the_root_override_wins_over_the_directory_the_command_was_run_in() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    witness(&root);
    let elsewhere = tempfile::tempdir().unwrap();

    let out = Command::new(env!(concat!("CARGO_BIN_EXE_", "mock")))
        .arg("locate")
        .current_dir(elsewhere.path())
        .env("MOCK_ROOT", &root)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("root={}\n", root.display())),
        "the override was ignored"
    );
}

#[test]
fn a_directory_in_no_repo_at_all_is_refused_and_the_message_names_both_reasons() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap();
    // the precondition, established rather than assumed: on a machine whose
    // TMPDIR sits inside a checkout the walk would find a real repo and the
    // refusal would never happen, and a test that shrugged at that would pass
    // having asserted nothing.
    let mut p = dir.as_path();
    loop {
        assert!(
            !p.join(".git").exists(),
            "TMPDIR is inside a repository ({}), so this cannot test a refusal",
            p.display()
        );
        match p.parent() {
            Some(up) => p = up,
            None => break,
        }
    }

    let out = Command::new(env!(concat!("CARGO_BIN_EXE_", "mock")))
        .arg("locate")
        .current_dir(&dir)
        .env_remove("MOCK_ROOT")
        .output()
        .unwrap();
    assert!(!out.status.success(), "a directory in no repo was accepted");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(".git"), "{err}");
    assert!(err.contains("MOCK_ROOT is unset"), "{err}");

    // and with the override set and wrong, the same refusal says so instead of
    // telling the operator the variable they just exported is unset.
    let out = Command::new(env!(concat!("CARGO_BIN_EXE_", "mock")))
        .arg("locate")
        .current_dir(&dir)
        .env("MOCK_ROOT", "/definitely/not/a/directory/xyzzy")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/definitely/not/a/directory/xyzzy"), "{err}");
    assert!(err.contains("not a directory"), "{err}");
    assert!(!err.contains("is unset"), "{err}");
}
