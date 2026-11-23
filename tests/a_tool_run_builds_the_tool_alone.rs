//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A run of one tool builds that tool and none of the repo's lints, also in a tree where no whole
//! build has run yet.
//!
//! The fixture holds one tool and one lint that does not compile, and no record of a whole build.
//! A run of the tool that compiled the lint would fail on it, so the run passing is the proof
//! that the lint was left out. The control is `lints`, which loads the whole pack: it has to fail
//! on the same lint, or the lint is not broken and the case proves nothing.
//!
//! The build goes into the target directory this test was built in, so the dependencies it
//! shares with every other build are compiled once. The remote is fixed, so each run of this
//! test reuses one generated crate rather than leaving another behind.

#[path = "common/ambient.rs"]
mod ambient;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dep_spec() -> String {
    let lint_rules = mockspace_manifest::toml_text::basic_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/lint-rules"
    ));
    format!("{{ package = \"mockspace-lint-rules\", path = {lint_rules} }}")
}

/// The target directory this test binary was built in: the engine binary sits in its profile
/// directory.
fn this_target() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_mockspace"))
        .parent()
        .and_then(Path::parent)
        .expect("the engine binary sits in a profile directory of a target")
        .to_path_buf()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").args(args).current_dir(dir).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn fixture(root: &Path) -> PathBuf {
    let mock = root.join("mock");
    let tool = mock.join("tools").join("greet");
    fs::create_dir_all(tool.join("src")).unwrap();
    fs::create_dir_all(mock.join("lints")).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["remote", "add", "origin", "ssh://git@example.invalid/o/a-tool-run-builds-the-tool-alone.git"]);
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    fs::write(
        tool.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-greet\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(
        tool.join("src").join("lib.rs"),
        r#"use mockspace::tool::{NotALint, Purpose, Tool, ToolContext, ToolReport};
pub struct T;
impl Tool for T {
    fn name(&self) -> &'static str { "greet" }
    fn description(&self) -> &'static str { "a probe tool" }
    fn purpose(&self) -> Purpose { Purpose::Check(NotALint::NoFailingCase) }
    fn run(&self, _: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("the tool ran", 1)
    }
}

mockspace::lint_pack! {
    tools: [T],
}
"#,
    )
    .unwrap();
    fs::write(mock.join("lints").join("broken.rs"), "this lint does not compile\n").unwrap();
    mock
}

fn engine(mock: &Path, args: &[&str]) -> Output {
    let dep = dep_spec();
    let mut all = vec!["--mockspace-lint-rules-dep", dep.as_str()];
    all.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(&all)
        .env("CARGO_TARGET_DIR", this_target())
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env("GIT_CEILING_DIRECTORIES", mock.parent().and_then(Path::parent).unwrap_or(mock))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(mock)
        .output()
        .expect("the binary runs")
}

#[test]
#[ignore = "the_mockspace_real_cargo_end_to_end_tests_are_ignored_for_th: runs a cargo build"]
fn a_tool_runs_in_a_tree_whose_lint_does_not_compile() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["greet"]);
    let err = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(0), "the tool must run without building the lint:\n{err}");
    assert!(err.contains("the tool ran"), "{err}");
}

#[test]
#[ignore = "the_mockspace_real_cargo_end_to_end_tests_are_ignored_for_th: runs a cargo build"]
fn the_control_a_whole_load_fails_on_the_same_lint() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["lints"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(0), "the lint must be broken, or the case above proves nothing:\n{err}");
    assert!(err.contains("BLOCKED"), "{err}");
}
