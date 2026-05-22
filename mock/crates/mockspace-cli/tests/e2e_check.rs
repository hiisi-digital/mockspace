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

// ---- check --json output mode --------------------------------------------

/// Run `cargo mock check --json --gate <gate>` against the fixture
/// and capture stdout. The JSON branch short-circuits the
/// human-readable path; empty findings serialise as `[]`. Same
/// exit-zero assertion as the stdout helper.
fn capture_check_json_stdout(fixture: &MockspaceFixture, gate: &str) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--json")
        .arg("--gate")
        .arg(gate)
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check --json");
    assert!(
        output.status.success(),
        "mock check --json exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("check --json stdout is UTF-8")
}

#[test]
fn check_json_on_empty_fixture_at_commit_gate() {
    // Empty fixture, JSON output. Golden is the canonical empty
    // array. Catches drift in the JSON branch's empty-case
    // handling and validates the --json flag plumbing.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_check_json_stdout(&fixture, "commit");
    assert_matches_golden("check_empty_fixture_commit_gate_json", &stdout);
}

#[test]
fn check_json_on_rust_crate_with_violations_at_commit_gate() {
    // Build a synthetic cargo workspace with a single member crate
    // whose `lib.rs` carries known violations (bare `usize` return,
    // bare `u64` pub field). The engine produces a non-empty
    // findings array. Golden pins the JSON shape: lint_name,
    // severity (lowercase via serde), message, span (file, line,
    // column ranges).
    //
    // KNOWN LIMITATION: at the current engine version every finding
    // reports `start_line: 1, start_column: 1, end_line: 1`. The
    // engine's per-lint span computation does not yet locate the
    // exact violation site; it reports the file-start position. The
    // `pub items: u64` violation is on line 3 of the source but the
    // golden encodes line 1; this is the engine's behaviour today,
    // not the CLI's. Future engine work that refines span precision
    // will require regenerating this golden, and the diff will
    // *look* like a regression while actually being a fix. Anyone
    // landing on a future drift should read this comment first.
    //
    // KNOWN ORDER: findings emit in `engine.run`'s dispatch order,
    // not alphabetical-by-lint-name. The golden currently shows
    // `no-public-raw-field`, then `no-bare-numeric` x2, then
    // `no-manual-id` because that is the order the engine emits
    // (catalog instantiation order applied to each document). Any
    // reordering of catalog registrations or dispatch will reorder
    // the golden.
    //
    // The golden is otherwise the point of the test: any wire-shape
    // drift visible to JSON consumers (editor integrations, CI
    // dashboards) flags here.
    let lib_rs = "pub fn count() -> usize { 42 }\npub struct Bag {\n    pub items: u64,\n}\n";
    let fixture = MockspaceFixture::new()
        .with_rust_crate("probe", lib_rs)
        .build()
        .expect("fixture");
    // The probe crate's `pub fn count() -> usize` and `pub items:
    // u64` trip Error-severity lints (no-bare-numeric et al), so
    // `mock check` exits non-zero. Use a custom invocation that
    // accepts FAILURE and still captures stdout.
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--json")
        .arg("--gate")
        .arg("commit")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check --json");
    // Error severity at commit gate means exit code FAILURE; verify
    // we got the expected non-zero exit without panicking the test.
    assert!(
        !output.status.success(),
        "expected non-zero exit because of Error findings; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("check --json stdout is UTF-8");
    assert_matches_golden("check_rust_crate_violations_commit_gate_json", &stdout);
}

