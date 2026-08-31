//! End-to-end integration tests for the `mock` binary.
//!
//! Each test builds the CLI as a process fixture via `assert_cmd` and
//! drives it against a fresh `MockspaceFixture` (from the
//! `mockspace-test-fixtures` crate) so no state leaks between cases.
//! The bootstrap module's unit tests already cover the per-function
//! behaviour matrix; this suite verifies the wiring through the
//! `clap` parser + `main` dispatch + stdout / stderr + exit-code
//! surface.
//!
//! Tests that need filesystem state pass `--repo-root <fixture-path>`
//! so they never touch the actual working directory. Tests that don't
//! need filesystem state (e.g. `explain` against the catalog defaults
//! when no user TOML is required) omit the flag and let the CLI
//! default to cwd.

use std::fs;

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;
use predicates::prelude::*;

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
    // clap-derive auto-generates `--version` from the `[package]
    // version` in mockspace-cli's Cargo.toml. Asserts both
    // exit-zero and the canonical `mock <semver>` stdout shape.
    // Loose contains-match keeps the assertion stable across
    // release bumps; tight enough to catch clap-derive dropping
    // the binary-name prefix.
    mock()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("mock "));
}

// ---- status --------------------------------------------------------------

#[test]
fn status_reports_not_installed_in_fresh_directory() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not installed"))
        .stdout(predicate::str::contains("mock/ directory       : no"))
        .stdout(predicate::str::contains("cargo alias `mock`    : no"))
        .stdout(predicate::str::contains("core.hooksPath set    : no"));
}

#[test]
fn status_reports_fully_adopted_after_install_with_mock_dir() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .with_install()
        .build()
        .expect("fixture");
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fully adopted"))
        .stdout(predicate::str::contains("yes"));
}

// ---- install / uninstall / refresh ---------------------------------------

#[test]
fn install_then_status_reports_installed_state() {
    // Install creates `mock/target/hooks/` as a side effect of
    // writing the hook scripts, which makes the `has_mock_dir`
    // signal true. So after install, status reports fully adopted
    // even though the test fixture did not pre-create `mock/`.
    // This codifies the observable post-install shape.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("installed"));
    mock()
        .arg("status")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("mock/ directory       : yes"))
        .stdout(predicate::str::contains("cargo alias `mock`    : yes"))
        .stdout(predicate::str::contains("core.hooksPath set    : yes"));
}

#[test]
fn install_creates_cargo_alias_file() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();
    let cargo_config = fixture.path().join(".cargo").join("config.toml");
    assert!(cargo_config.is_file(), "cargo alias config should exist");
    let body = fs::read_to_string(&cargo_config).unwrap();
    assert!(
        body.contains("mock") && body.contains("run --manifest-path"),
        "cargo config should carry the alias entry: {body:?}"
    );
}

#[test]
fn install_creates_hook_scripts() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();
    let hooks_dir = fixture.path().join("mock").join("target").join("hooks");
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
    // First "install" via the fixture builder; second via the CLI
    // should observe AlreadyInstalled.
    let fixture = MockspaceFixture::new()
        .with_install()
        .build()
        .expect("fixture");
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already installed"));
}

#[test]
fn uninstall_round_trip_via_cli() {
    let fixture = MockspaceFixture::new()
        .with_install()
        .build()
        .expect("fixture");
    mock()
        .arg("uninstall")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("removed"));
    let hooks_dir = fixture.path().join("mock").join("target").join("hooks");
    assert!(!hooks_dir.join("pre-commit").exists());
    assert!(!hooks_dir.join("pre-push").exists());
}

#[test]
fn uninstall_on_clean_directory_reports_not_installed() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("uninstall")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("was not installed"));
}

#[test]
fn refresh_acts_as_install_when_nothing_present() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("refresh")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("refreshed"));
}

#[test]
fn refresh_repairs_drifted_hook_body() {
    let fixture = MockspaceFixture::new()
        .with_install()
        .build()
        .expect("fixture");
    let pre_commit = fixture
        .path()
        .join("mock")
        .join("target")
        .join("hooks")
        .join("pre-commit");
    fs::write(&pre_commit, "#!/bin/sh\necho stale\n").unwrap();
    mock()
        .arg("refresh")
        .arg("--repo-root")
        .arg(fixture.path())
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
        .arg("no-bare-vec")
        .assert()
        .success()
        .stdout(predicate::str::contains("lint: no-bare-vec"))
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
        .stderr(predicate::str::contains(
            "not found in the registered catalog",
        ));
}

#[test]
fn explain_picks_up_per_lint_toml_override() {
    let fixture = MockspaceFixture::new()
        .with_lints_toml(
            r#"
[lints.no-bare-vec.scope]
exempt_paths = ["**/cli_fixture/**"]
"#,
        )
        .build()
        .expect("fixture");
    mock()
        .arg("explain")
        .arg("no-bare-vec")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Layer 4: per-lint TOML"))
        .stdout(predicate::str::contains("**/cli_fixture/**"));
}

#[test]
fn explain_picks_up_workspace_defaults_at_layer_3() {
    // The `[defaults]` block in lints.toml populates Layer 3 of
    // the cascade. By design the block is flat and merges onto
    // the config side; `[defaults.scope]` reads as a config key
    // named `scope` carrying a table value, not a scope-side
    // override. This codifies the as-shipped contract.
    let fixture = MockspaceFixture::new()
        .with_lints_toml(
            r#"
[defaults]
visibility = "all"
"#,
        )
        .build()
        .expect("fixture");
    mock()
        .arg("explain")
        .arg("no-bare-vec")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Layer 3: workspace defaults (`[defaults]` in lints.toml)",
        ))
        .stdout(predicate::str::contains("config.visibility = \"all\""))
        .stdout(predicate::str::contains(
            "config.visibility = \"all\" (Layer 3: workspace defaults)",
        ));
}

#[test]
fn explain_per_lint_toml_wins_over_workspace_defaults() {
    // Layer 4 ranks above Layer 3 in the cascade. When both
    // `[defaults]` and `[lints.<name>.config]` set the same key,
    // the per-lint value wins. This pins the cascade ordering.
    let fixture = MockspaceFixture::new()
        .with_lints_toml(
            r#"
[defaults]
visibility = "all"

[lints.no-bare-vec.config]
visibility = "crate"
"#,
        )
        .build()
        .expect("fixture");
    mock()
        .arg("explain")
        .arg("no-bare-vec")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "config.visibility = \"crate\" (Layer 4: per-lint TOML)",
        ));
}

#[test]
fn explain_warns_on_unparseable_user_toml_but_still_runs() {
    // The fixture builder writes lints.toml contents verbatim with
    // no validation, so feeding it garbage exercises the CLI's
    // warn-and-proceed path against an unparseable user TOML.
    let fixture = MockspaceFixture::new()
        .with_lints_toml("<<<not toml>>>")
        .build()
        .expect("fixture");
    mock()
        .arg("explain")
        .arg("no-bare-vec")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"))
        .stdout(predicate::str::contains("lint: no-bare-vec"));
}

// ---- check ---------------------------------------------------------------

#[test]
fn check_help_lists_all_flags() {
    // Help text smoke: catches drift if any of the flags added
    // across PRs #97 (--gate, --repo-root), #102 (--json), and
    // this slice (--surface) are renamed, removed, or accidentally
    // hidden from clap output. The empty-fixture and explain tests
    // already exercise the semantics; this just pins the surface a
    // user sees on `mock check --help`.
    mock()
        .arg("check")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--gate"))
        .stdout(predicate::str::contains("commit"))
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("push"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--repo-root"))
        .stdout(predicate::str::contains("--surface"))
        .stdout(predicate::str::contains("local"))
        .stdout(predicate::str::contains("ci"))
        .stdout(predicate::str::contains("editor"));
}

#[test]
fn check_accepts_surface_ci_flag() {
    // Smoke: --surface ci passes through clap parsing and the
    // engine accepts the resulting `RunSurface::Ci` value. Empty
    // fixture means zero findings regardless of surface, so the
    // assertion is just exit-zero with the "no findings" line.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("check")
        .arg("--surface")
        .arg("ci")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no findings"));
}

#[test]
fn check_rejects_invalid_surface_value() {
    // clap's value-enum validation should reject anything outside
    // the three RunSurface variants. The error message should
    // name the offending value and the allowed values; if a
    // future clap version changes that contract, this test
    // catches the surprise before it reaches users.
    mock()
        .arg("check")
        .arg("--surface")
        .arg("nonsense")
        .assert()
        .failure()
        .stderr(predicate::str::contains("nonsense"))
        .stderr(predicate::str::contains("possible values"));
}

#[test]
fn check_on_empty_fixture_reports_no_findings() {
    // Empty fixture has no Rust source, so the engine scopes a
    // zero-document project and produces zero findings. Exit code
    // is success.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("check")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no findings"));
}

#[test]
fn check_accepts_explicit_gate_flag() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("check")
        .arg("--gate")
        .arg("push")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no findings"));
}
