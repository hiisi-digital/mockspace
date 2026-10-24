//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A git repository with a mock directory and one bench arm, as `(root, mock)`.
fn repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let st = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(st.success());
    let mock = root.join("mock");
    write(
        &mock.join("benches/sample/bench.toml"),
        "name = \"sample\"\n",
    );
    write(
        &mock.join("benches/sample/arms/a/src/lib.rs"),
        "pub fn f() {}\n",
    );
    (tmp, root, mock)
}

fn now(root: &Path, mock: &Path) -> String {
    digest(root, mock, &mock.join("benches")).expect("a git repository has a digest")
}

fn moved_by(edit: impl FnOnce(&Path, &Path)) -> bool {
    let (_t, root, mock) = repo();
    let before = now(&root, &mock);
    edit(&root, &mock);
    before != now(&root, &mock)
}

#[path = "test_benches_decide_tests.rs"]
mod decide;

#[path = "test_benches_inputs_tests.rs"]
mod inputs;

#[path = "test_benches_lockfile_tests.rs"]
mod lockfile;

#[path = "test_benches_scheme_tests.rs"]
mod scheme;
