//! Golden-result end-to-end tests for `cargo mock explain`.
//!
//! Pattern: invoke the CLI against a fixture, capture stdout, compare
//! against a checked-in golden snapshot under `tests/goldens/`. On
//! mismatch the test fails and prints the diff hint. To intentionally
//! refresh a snapshot after a format change, run:
//!
//! ```text
//! MOCKSPACE_UPDATE_GOLDENS=1 cargo test -p mockspace-cli --test e2e_explain
//! ```
//!
//! The env-var knob is the standard regenerate-on-purpose convention.
//! Without it, the suite is read-only and CI-safe.
//!
//! This is the first slice of the golden-result e2e tree comparison
//! work (task #563). It starts with stdout-only goldens because the
//! explain output is structured, deterministic, and trivially compares
//! via a string match. Filesystem-tree goldens (walking the fixture's
//! resulting directory state) land in a follow-up; the harness shape
//! generalises cleanly.

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;

mod common;
use common::assert_matches_golden;

/// Run `cargo mock explain <name>` against the fixture and capture
/// stdout as a UTF-8 string. Fails the test if the CLI exits non-zero
/// or emits non-UTF-8 bytes (neither is expected for the lints in the
/// catalog).
fn capture_explain_stdout(fixture: &MockspaceFixture, lint_name: &str) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("explain")
        .arg(lint_name)
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock explain");
    assert!(
        output.status.success(),
        "mock explain exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("explain stdout is UTF-8")
}

/// Run `cargo mock explain <name>` against the fixture and capture
/// stderr as a UTF-8 string. Asserts the CLI exited non-zero (this
/// helper is for the failure path; the lint-not-found case is the
/// canonical use). The fixture is optional because failure cases
/// like `LintNotFound` don't depend on any fixture TOML; pass `None`
/// to run without `--repo-root`.
fn capture_explain_failure_stderr(fixture: Option<&MockspaceFixture>, lint_name: &str) -> String {
    let mut command = Command::cargo_bin("mock").expect("cargo build provides the mock binary");
    command.arg("explain").arg(lint_name);
    if let Some(f) = fixture {
        command.arg("--repo-root").arg(f.path());
    }
    let output = command.output().expect("invoke mock explain");
    assert!(
        !output.status.success(),
        "mock explain unexpectedly succeeded; stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr).expect("explain stderr is UTF-8")
}

// ---- explain catalog defaults --------------------------------------------

#[test]
fn explain_no_bare_vec_against_catalog_defaults() {
    // No user TOML, no overrides; the cascade walk resolves entirely
    // from Layer 1 (catalog defaults). This is the most stable shape
    // to golden-test: the catalog defaults are pinned by the lint's
    // CatalogEntry, so any drift in this golden flags an unintended
    // change to either the entry or the renderer.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_explain_stdout(&fixture, "no-bare-vec");
    assert_matches_golden("explain_no_bare_vec_catalog_defaults", &stdout);
}

// ---- explain Layer 4 (per-lint TOML override) ----------------------------

#[test]
fn explain_no_bare_vec_with_per_lint_toml_override() {
    // A user-authored lints.toml drives Layer 4. The override changes
    // scope.exempt_paths; Final values for that key resolves to
    // Layer 4, while the unchanged config.* keys still resolve to
    // Layer 1. This golden pins both the override-applies semantics
    // and the layer-precedence ordering.
    let fixture = MockspaceFixture::new()
        .with_lints_toml(
            r#"
[lints.no-bare-vec.scope]
exempt_paths = ["**/golden_fixture/**"]
"#,
        )
        .build()
        .expect("fixture");
    let stdout = capture_explain_stdout(&fixture, "no-bare-vec");
    assert_matches_golden("explain_no_bare_vec_per_lint_toml_override", &stdout);
}

// ---- explain Layer 3 (workspace defaults) --------------------------------

#[test]
fn explain_no_bare_vec_with_workspace_defaults() {
    // The `[defaults]` block populates Layer 3. By design the block
    // is flat and merges onto the config side; `[defaults] visibility
    // = "all"` overrides the catalog default Layer 1 value. The
    // golden captures both the Layer 3 contribution and the Final
    // value resolving to Layer 3.
    let fixture = MockspaceFixture::new()
        .with_lints_toml(
            r#"
[defaults]
visibility = "all"
"#,
        )
        .build()
        .expect("fixture");
    let stdout = capture_explain_stdout(&fixture, "no-bare-vec");
    assert_matches_golden("explain_no_bare_vec_workspace_defaults", &stdout);
}

// ---- explain unknown-lint error path -------------------------------------

#[test]
fn explain_unknown_lint_stderr_diagnostic() {
    // The unknown-lint error path surfaces ExplainError::LintNotFound
    // on stderr with a structured message that names what the user
    // typed back. This golden pins the error format so any drift in
    // the Display impl on ExplainError flags here.
    let stderr = capture_explain_failure_stderr(None, "no-such-lint-ever");
    assert_matches_golden("explain_unknown_lint_stderr", &stderr);
}
