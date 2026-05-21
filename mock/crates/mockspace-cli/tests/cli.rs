//! End-to-end integration tests for the `mock` binary.
//!
//! Each test builds the CLI as a process fixture via `assert_cmd` and
//! drives it against a fresh `tempfile::TempDir` so no state leaks
//! between cases. The bootstrap module's unit tests already cover the
//! per-function behaviour matrix; this suite verifies the wiring
//! through the `clap` parser + `main` dispatch + stdout / stderr +
//! exit-code surface.
//!
//! All tests pass `--repo-root <tempdir>` so they never touch the
//! actual working directory. The fixtures stay independent of cargo's
//! own `target/` tree.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn mock() -> Command {
    Command::cargo_bin("mock").expect("cargo build provides the mock binary")
}

// ---- help / version ------------------------------------------------------

#[test]
fn help_lists_all_subcommands() {
    mock()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("uninstall"))
        .stdout(predicate::str::contains("refresh"))
        .stdout(predicate::str::contains("explain"));
}

#[test]
fn version_flag_succeeds() {
    mock().arg("--version").assert().success();
}

// ---- status --------------------------------------------------------------

#[test]
fn status_reports_not_installed_in_fresh_directory() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not installed"))
        .stdout(predicate::str::contains("mock/ directory       : no"))
        .stdout(predicate::str::contains("cargo alias `mock`    : no"))
        .stdout(predicate::str::contains("core.hooksPath set    : no"));
}

#[test]
fn status_reports_fully_adopted_after_install_with_mock_dir() {
    let tmp = TempDir::new().unwrap();
    // The bootstrap install creates the cargo alias + hooks; the
    // `mock/` directory is part of the consumer's repo skeleton and
    // not created by bootstrap, so the test fixture pre-creates it.
    fs::create_dir_all(tmp.path().join("mock")).unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fully adopted"))
        .stdout(predicate::str::contains("yes"));
}

// ---- install / uninstall / refresh ---------------------------------------

#[test]
fn install_then_status_reports_installed_state() {
    let tmp = TempDir::new().unwrap();
    // Install creates `mock/target/hooks/` as a side effect of
    // writing the hook scripts, which makes the `has_mock_dir`
    // signal true. So after install, status reports fully adopted
    // even though the test fixture did not pre-create `mock/`.
    // This codifies the observable post-install shape.
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("installed"));
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("mock/ directory       : yes"))
        .stdout(predicate::str::contains("cargo alias `mock`    : yes"))
        .stdout(predicate::str::contains("core.hooksPath set    : yes"));
}

#[test]
fn install_creates_cargo_alias_file() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    let cargo_config = tmp.path().join(".cargo").join("config.toml");
    assert!(cargo_config.is_file(), "cargo alias config should exist");
    let body = fs::read_to_string(&cargo_config).unwrap();
    assert!(
        body.contains("mock") && body.contains("run --manifest-path"),
        "cargo config should carry the alias entry: {body:?}"
    );
}

#[test]
fn install_creates_hook_scripts() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    let hooks_dir = tmp.path().join("mock").join("target").join("hooks");
    assert!(
        hooks_dir.join("pre-commit").is_file(),
        "pre-commit hook should exist"
    );
    assert!(
        hooks_dir.join("pre-push").is_file(),
        "pre-push hook should exist"
    );
}

#[test]
fn install_is_idempotent_via_cli() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("installed"));
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already installed"));
}

#[test]
fn uninstall_round_trip_via_cli() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    mock()
        .arg("uninstall")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
    let hooks_dir = tmp.path().join("mock").join("target").join("hooks");
    assert!(!hooks_dir.join("pre-commit").exists());
    assert!(!hooks_dir.join("pre-push").exists());
}

#[test]
fn uninstall_on_clean_directory_reports_not_installed() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("uninstall")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("was not installed"));
}

#[test]
fn refresh_acts_as_install_when_nothing_present() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("refresh")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("refreshed"));
}

#[test]
fn refresh_repairs_drifted_hook_body() {
    let tmp = TempDir::new().unwrap();
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    let pre_commit = tmp
        .path()
        .join("mock")
        .join("target")
        .join("hooks")
        .join("pre-commit");
    fs::write(&pre_commit, "#!/bin/sh\necho stale\n").unwrap();
    mock()
        .arg("refresh")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success();
    let body = fs::read_to_string(&pre_commit).unwrap();
    assert!(
        body.contains("cargo mock check --gate commit"),
        "refresh should restore canonical body, got: {body:?}"
    );
}

// ---- explain -------------------------------------------------------------

#[test]
fn explain_known_lint_prints_cascade_report() {
    mock()
        .arg("explain")
        .arg("no-bare-numeric")
        .assert()
        .success()
        .stdout(predicate::str::contains("lint: no-bare-numeric"))
        .stdout(predicate::str::contains("Cascade layers:"))
        .stdout(predicate::str::contains("Layer 1: catalog defaults"))
        .stdout(predicate::str::contains("Final values:"));
}

#[test]
fn explain_unknown_lint_fails_with_clear_error() {
    mock()
        .arg("explain")
        .arg("no-such-lint-ever")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found in the registered catalog"));
}

#[test]
fn explain_picks_up_per_lint_toml_override() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("lints.toml"),
        r#"
[lints.no-bare-numeric.scope]
exempt_paths = ["**/cli_fixture/**"]
"#,
    )
    .unwrap();
    mock()
        .arg("explain")
        .arg("no-bare-numeric")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Layer 4: per-lint TOML"))
        .stdout(predicate::str::contains("**/cli_fixture/**"));
}

#[test]
fn explain_warns_on_unparseable_user_toml_but_still_runs() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("lints.toml"), "<<<not toml>>>").unwrap();
    mock()
        .arg("explain")
        .arg("no-bare-numeric")
        .arg("--repo-root")
        .arg(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("lint: no-bare-numeric"));
}
