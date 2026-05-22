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
use std::path::{Path, PathBuf};

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

/// Resolve the path to a checked-in golden file under
/// `<crate>/tests/goldens/<name>.golden`. Centralised so consumers
/// don't repeat the `CARGO_MANIFEST_DIR` join idiom.
fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(format!("{name}.golden"))
}

/// Compare `actual` against the checked-in golden at `<name>.golden`.
/// On `MOCKSPACE_UPDATE_GOLDENS=1`, writes `actual` to the golden path
/// (creating the parent directory if needed) and passes. Otherwise
/// reads the golden and asserts byte-equality, failing with a diff
/// hint pointing at the regenerate knob.
fn assert_matches_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    let update = std::env::var("MOCKSPACE_UPDATE_GOLDENS")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false);

    if update {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create goldens directory");
        }
        std::fs::write(&path, actual).expect("write golden");
        return;
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => panic!(
            "golden `{name}` missing at {}: {e}.\n\
             To create it, rerun with MOCKSPACE_UPDATE_GOLDENS=1",
            path.display()
        ),
    };

    if expected != actual {
        panic!(
            "golden `{name}` does not match.\n\
             Expected (from {}):\n{expected}\n\
             ---\n\
             Actual:\n{actual}\n\
             ---\n\
             To accept the new output, rerun with MOCKSPACE_UPDATE_GOLDENS=1",
            path.display()
        );
    }
}

// ---- explain catalog defaults --------------------------------------------

#[test]
fn explain_no_bare_numeric_against_catalog_defaults() {
    // No user TOML, no overrides; the cascade walk resolves entirely
    // from Layer 1 (catalog defaults). This is the most stable shape
    // to golden-test: the catalog defaults are pinned by the lint's
    // CatalogEntry, so any drift in this golden flags an unintended
    // change to either the entry or the renderer.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_explain_stdout(&fixture, "no-bare-numeric");
    assert_matches_golden("explain_no_bare_numeric_catalog_defaults", &stdout);
}
