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
    // not alphabetical-by-lint-name. After #568, the preset-replaced
    // lints (no-bare-numeric, no-public-raw-field) are no longer
    // auto-registered; this fixture triggers only the bespoke
    // `no-bare-vec` and `no-manual-id` lints that ship as catalog
    // entries today.
    //
    // The golden is otherwise the point of the test: any wire-shape
    // drift visible to JSON consumers (editor integrations, CI
    // dashboards) flags here.
    let lib_rs = "pub fn handle(items: Vec<u8>) {}\npub struct Bag {\n    pub items: Vec<u32>,\n}\n";
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

#[test]
fn check_surfaces_config_errors_visibly_for_invalid_visibility_variant() {
    // Issue #106: writing `visibility = "all"` in lints.toml (or any
    // [defaults] / per-lint override) is not a valid `Visibility`
    // enum variant (only `any` and `public` exist). Before the fix,
    // the ConfigError was silently swallowed: the affected lint(s)
    // dropped from the active set, `mock check` ran the remainder,
    // and the user saw "no findings" with no clue why. After the
    // fix, the CLI renders each ConfigError on its own line on
    // stderr and exits FAILURE so the silent-vs-actual mismatch
    // can never happen.
    //
    // The golden captures the stderr shape so any drift in the
    // ConfigError Display impl, the file/line/col fallback, or
    // the trailing summary line flags here.
    let fixture = MockspaceFixture::new()
        .with_lints_toml("[defaults]\nvisibility = \"all\"\n")
        .build()
        .expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check");
    assert!(
        !output.status.success(),
        "expected non-zero exit because config errors dropped lints; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Replace the tempdir-prefixed lints.toml path with a stable
    // placeholder so the golden is reproducible across runs and
    // machines. The fixture path lives under a random `/var/...`
    // or `/tmp/...` directory created by the builder.
    let stderr = String::from_utf8(output.stderr).expect("check stderr is UTF-8");
    let fixture_path = fixture.path().display().to_string();
    let stable = stderr.replace(&fixture_path, "<FIXTURE>");
    assert_matches_golden("check_config_errors_visible_for_invalid_visibility", &stable);
}

#[test]
fn check_human_on_rust_crate_with_violations_at_commit_gate() {
    // Parallel to the JSON failing-fixture test: same probe crate
    // shape, same engine output, but captures the human-readable
    // diagnostic format (`<file>:<line>:<col>: [<severity>] <name>:
    // <message>` per finding, no JSON envelope). The human path is
    // what pre-commit hook users actually see; this pins it.
    //
    // KNOWN LIMITATION: same as the JSON test above. Engine spans
    // currently report `1:1` for every finding regardless of the
    // actual line in the source. Future engine work refining span
    // precision will regenerate this golden. The order is also
    // engine emission order, not alphabetical.
    let lib_rs = "pub fn handle(items: Vec<u8>) {}\npub struct Bag {\n    pub items: Vec<u32>,\n}\n";
    let fixture = MockspaceFixture::new()
        .with_rust_crate("probe", lib_rs)
        .build()
        .expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check");
    assert!(
        !output.status.success(),
        "expected non-zero exit because of Error findings; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("check stdout is UTF-8");
    assert_matches_golden("check_rust_crate_violations_commit_gate", &stdout);
}

// ---- check --fix and --dry-run plumbing ----------------------------------

#[test]
fn check_fix_on_empty_fixture_prints_zero_applied_summary() {
    // Empty fixture: zero findings, zero fixable. `--fix` exits
    // SUCCESS and prints the "applied 0 fix(es)..." summary line
    // after the empty-findings notice. Verifies the fix-path
    // plumbing reaches `apply_fixes` without short-circuiting and
    // that the summary tally renders even when the plan is empty.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--fix")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check --fix");
    assert!(
        output.status.success(),
        "empty fixture with --fix must exit success; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("check --fix stdout is UTF-8");
    assert!(
        stdout.contains("applied 0 fix"),
        "expected applied-tally line in stdout, got: {stdout}"
    );
}

#[test]
fn check_dry_run_on_empty_fixture_prints_no_fixable_findings() {
    // Empty fixture + `--dry-run`. The fix path runs but does not
    // write; the plan is empty so `render_unified_diff` returns an
    // empty string and the helper prints "no fixable findings".
    // Exit code still tracks the gate evaluation (success).
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--dry-run")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check --dry-run");
    assert!(
        output.status.success(),
        "empty fixture with --dry-run must exit success; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("check --dry-run stdout is UTF-8");
    assert!(
        stdout.contains("no fixable findings"),
        "expected dry-run summary in stdout, got: {stdout}"
    );
}

#[test]
fn check_fix_on_rust_crate_with_violations_keeps_findings_visible() {
    // Findings with no `suggestion.fix` are advisory-only. After
    // #568 the probe crate triggers `no-bare-vec` (bespoke, still
    // auto-registered). The lint does not carry an auto-fix
    // suggestion, so `--fix` is a no-op on edits but the findings
    // still print and the exit code still reflects the gate
    // failure. Verifies `--fix` does not silence Error-severity
    // diagnostics.
    let lib_rs = "pub fn handle(items: Vec<u8>) {}\npub struct Bag {\n    pub items: Vec<u32>,\n}\n";
    let fixture = MockspaceFixture::new()
        .with_rust_crate("probe", lib_rs)
        .build()
        .expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--fix")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check --fix");
    assert!(
        !output.status.success(),
        "Error-severity findings must still fail the gate even with --fix; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("check --fix stdout is UTF-8");
    assert!(
        stdout.contains("no-bare-vec"),
        "findings must still print before the fix summary; stdout: {stdout}"
    );
    assert!(
        stdout.contains("applied"),
        "fix-summary tally must appear after the diagnostics; stdout: {stdout}"
    );
}

// ---- preset-as-catalog opt-in via extends (#611) -------------------------

#[test]
fn check_opts_into_preset_replaced_lint_via_extends() {
    // Closes the loop on #568 + #611: post #189 removal, the 15
    // preset-replaced lints are unreachable from consumer TOML unless
    // the consumer adds `extends = "mockspace::<name>"`. This e2e
    // verifies the opt-in works end-to-end through the CLI: a probe
    // crate triggers `no-bare-numeric` (a preset-replaced lint), the
    // user's lints.toml carries the `extends` shorthand, and the
    // engine surfaces a finding under the synthesised name.
    let lib_rs = "pub fn count() -> u64 { 42 }\n";
    let lints_toml = r#"
[lints.no-bare-numeric]
extends = "mockspace::no-bare-numeric"
"#;
    let fixture = MockspaceFixture::new()
        .with_lints_toml(lints_toml)
        .with_rust_crate("probe", lib_rs)
        .build()
        .expect("fixture");
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("check")
        .arg("--gate")
        .arg("commit")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The synthesised lint should surface a finding by name.
    assert!(
        stdout.contains("no-bare-numeric"),
        "expected `no-bare-numeric` finding in stdout; got:\nstdout: {stdout}\nstderr: {stderr}"
    );
    // No config errors expected: the extends path resolves cleanly.
    assert!(
        !stderr.contains("config error"),
        "expected no config errors when opting into a first-party preset; \
         stderr: {stderr}"
    );
}
