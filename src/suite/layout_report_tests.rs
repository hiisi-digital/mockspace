//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::path::Path;
use std::process::Command;

use super::*;

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .current_dir(root)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

/// A git repository with one workspace member, `solo`, under `mock/`.
fn repo(_tag: &str) -> (tempfile::TempDir, Config) {
    let guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = guard.path().to_path_buf();
    let krate = root.join("mock/solo");
    std::fs::create_dir_all(krate.join("src")).unwrap();
    std::fs::write(
        root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = [\"solo\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        krate.join("Cargo.toml"),
        "[package]\nname = \"solo\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(krate.join("src/lib.rs"), "").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "mock/Cargo.toml", "mock/solo/Cargo.toml", "mock/solo/src/lib.rs"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let mut cfg = Config::from_dir(Path::new("/nonexistent-mock-dir"));
    cfg.repo_root = root.clone();
    cfg.mock_dir = root.join("mock");
    (guard, cfg)
}

#[test]
fn a_member_that_is_not_in_the_workspace_fails_and_a_real_one_does_not() {
    let (_guard, cfg) = repo("unknown");
    let code = run(&cfg, &["nope".to_string()]);
    let control = run(&cfg, &["solo".to_string()]);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(control, ExitCode::SUCCESS);
}
