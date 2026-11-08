//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::path::{Path, PathBuf};

use mockspace_lint_rules::LintMode;

use super::{of_run, reduce};
use crate::entry::registry_scope::Changed;

const REPO: &str = "/r";

fn files(list: &[&str]) -> Changed {
    Changed::Files(list.iter().map(|f| Path::new(REPO).join(f)).collect::<Vec<PathBuf>>())
}

fn reduced(list: &[&str]) -> Option<Vec<String>> {
    reduce(&files(list), Path::new(REPO), Path::new("/r/mock"))
}

#[test]
fn the_files_come_back_relative_to_the_repository() {
    assert_eq!(
        reduced(&["content/a.toml", "mock/crates/x/src/lib.rs"]),
        Some(vec!["content/a.toml".to_string(), "mock/crates/x/src/lib.rs".to_string()])
    );
}

#[test]
fn a_change_to_the_lint_set_runs_every_lint() {
    for set in [
        "mockspace.toml",
        "mock/mockspace.toml",
        "mock/lints/a_lint.rs",
        "mock/tools/t/src/main.rs",
        "mock/Cargo.toml",
        "mock/Cargo.lock",
        "mock/rust-toolchain.toml",
    ] {
        assert_eq!(reduced(&["content/a.toml", set]), None, "{set} is the lint set");
    }
}

#[test]
fn a_crate_beside_the_lint_set_is_not_the_lint_set() {
    assert!(reduced(&["mock/lints_notes.md", "mock/crates/lints/src/lib.rs"]).is_some());
}

#[test]
fn a_run_git_could_not_list_runs_every_lint() {
    assert_eq!(reduce(&Changed::Unknown, Path::new(REPO), Path::new("/r/mock")), None);
    assert_eq!(reduce(&Changed::NotCommit, Path::new(REPO), Path::new("/r/mock")), None);
}

#[test]
fn only_the_commit_gate_narrows() {
    for mode in [LintMode::Push, LintMode::Build] {
        assert_eq!(of_run(mode, Path::new("/nonexistent"), Path::new("/nonexistent/mock")), None);
    }
}
