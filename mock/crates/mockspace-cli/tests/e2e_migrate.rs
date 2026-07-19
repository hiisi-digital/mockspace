//! Golden-result end-to-end tests for `cargo mock migrate`.
//!
//! Migrate is the v1-to-v2 transition guide: it walks
//! `mock/design_rounds/` for v1-shaped state and prints a per-round
//! migration plan plus the canonical human-side checklist (CI
//! workflow updates, prose grep-and-replace targets). These tests
//! pin the stdout shape so refactors that change the migration-
//! guidance vocabulary surface here first.
//!
//! Complements `e2e_install.rs` (filesystem-footprint), `e2e_check.rs`
//! (lint engine stdout), and `e2e_explain.rs` (cascade renderer).

use std::fs;

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;

mod common;
use common::assert_matches_golden;

/// Run `cargo mock migrate` against the fixture and capture stdout.
/// Replaces the fixture's tempdir path in the captured output with
/// a stable `<FIXTURE>` placeholder so the golden is reproducible
/// across runs and machines.
fn capture_migrate_stdout(fixture: &MockspaceFixture) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("migrate")
        .arg("--repo-root")
        .arg(fixture.path())
        .output()
        .expect("invoke mock migrate");
    assert!(
        output.status.success(),
        "mock migrate exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("migrate stdout is UTF-8");
    let fixture_path = fixture.path().display().to_string();
    stdout.replace(&fixture_path, "<FIXTURE>")
}

#[test]
fn migrate_on_bare_fixture_reports_no_v1_state() {
    // Bare fixture has no `mock/design_rounds/`. Migrate must
    // surface the no-v1-state line + the canonical post-script
    // checklist for hand-editing CI workflows and prose. Golden
    // pins this empty-state guidance.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let stdout = capture_migrate_stdout(&fixture);
    assert_matches_golden("migrate_bare_fixture", &stdout);
}

#[test]
fn migrate_on_fixture_with_v1_round_describes_it() {
    // Fixture with one v1-shaped round directory under
    // `mock/design_rounds/`. Migrate must classify the round as
    // empty (no CLs), describe the v2 target phase, and append the
    // canonical checklist. Golden pins the rendered shape.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    let round_dir = fixture
        .path()
        .join("mock")
        .join("design_rounds")
        .join("2026-04-25_test-round");
    fs::create_dir_all(&round_dir).expect("create v1 round dir");
    let stdout = capture_migrate_stdout(&fixture);
    assert_matches_golden("migrate_fixture_with_v1_round", &stdout);
}
