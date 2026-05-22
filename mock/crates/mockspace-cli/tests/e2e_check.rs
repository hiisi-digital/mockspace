//! Golden-result end-to-end tests for `cargo mock check`.
//!
//! Captures the engine-dispatch stdout against a checked-in
//! snapshot. Catches drift in the diagnostic format: the
//! `<file>:<line>:<col>: [<severity>] <name>: <message>` shape,
//! the empty-project `no findings at gate <Gate>` line, and how
//! the CLI renders exit-zero vs exit-failure runs.
//!
//! Complements `e2e_explain.rs` (cascade renderer drift) and
//! `e2e_install.rs` (filesystem-footprint drift). Together the
//! three e2e files cover the three primary CLI subcommands.

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;

mod common;
use common::assert_matches_golden;

/// Run `cargo mock check --gate <gate>` against the fixture and
/// capture stdout as a UTF-8 string. Asserts exit-zero (the
/// empty-project happy path); the failure-path capture helper
/// lands when a future fixture surfaces an actual lint violation.
fn capture_check_stdout(fixture: &MockspaceFixture, gate: &str) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg(gate)
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check");
    assert!(
        output.status.success(),
        "mock check exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("check stdout is UTF-8")
}

// ---- check on empty fixture ----------------------------------------------

#[test]
fn check_on_empty_fixture_at_commit_gate() {
    // Bare fixture, no Rust source under crates/. The engine
    // scopes zero documents and produces zero findings. Golden
    // captures the "no findings at gate Commit" line shape;
    // any drift to the default empty-result message flags here.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_check_stdout(&fixture, "commit");
    assert_matches_golden("check_empty_fixture_commit_gate", &stdout);
}

#[test]
fn check_on_empty_fixture_at_push_gate() {
    // Same shape against the strictest gate. The push-gate label
    // in the empty-output line is the only thing that should
    // differ between this and the commit-gate golden.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_check_stdout(&fixture, "push");
    assert_matches_golden("check_empty_fixture_push_gate", &stdout);
}

