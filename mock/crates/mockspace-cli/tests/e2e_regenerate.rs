//! End-to-end smoke tests for `cargo mock regenerate`.
//!
//! Covers the happy path (templates present, rendered output appears
//! under `docs/`) and the missing-template path (no templates means
//! the renderer surfaces `TemplateMissing` and the CLI exits FAILURE).
//! Per-template rendering semantics are exercised by the unit-test
//! suite in `mockspace_rs::render`; these tests verify the CLI
//! plumbing only (subcommand parsing, scope_walk integration, exit
//! code wiring, report printing).

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;
use std::fs;

fn mock() -> Command {
    Command::cargo_bin("mock").expect("cargo build provides the mock binary")
}

/// Drop the three mock-root templates under `<root>/mock/` with
/// minimal bodies suitable for verifying the rendered output paths.
fn write_root_templates(root: &std::path::Path) {
    let mock_dir = root.join("mock");
    fs::create_dir_all(&mock_dir).expect("mkdir mock/");
    for (name, body) in [
        ("DESIGN.md.tmpl", "design body"),
        ("PRINCIPLES.md.tmpl", "principles body"),
        ("WORKFLOW.md.tmpl", "workflow body"),
    ] {
        fs::write(mock_dir.join(name), body).expect("write template");
    }
}

#[test]
fn regenerate_errors_when_templates_missing() {
    // Bare fixture has no `mock/` dir; regenerate must surface the
    // TemplateMissing error path and exit FAILURE rather than
    // silently producing an empty docs/.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("regenerate")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .failure();
}

#[test]
fn regenerate_writes_three_mock_root_files() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .build()
        .expect("fixture");
    write_root_templates(fixture.path());

    mock()
        .arg("regenerate")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();

    let docs = fixture.path().join("docs");
    assert_eq!(
        fs::read_to_string(docs.join("DESIGN.md")).unwrap(),
        "design body"
    );
    assert_eq!(
        fs::read_to_string(docs.join("PRINCIPLES.md")).unwrap(),
        "principles body"
    );
    assert_eq!(
        fs::read_to_string(docs.join("WORKFLOW.md")).unwrap(),
        "workflow body"
    );
}

#[test]
fn regenerate_check_succeeds_when_disk_matches() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .build()
        .expect("fixture");
    write_root_templates(fixture.path());

    // Seed the docs/ tree.
    mock()
        .arg("regenerate")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();

    // Re-run with --check: disk matches render, exit SUCCESS.
    mock()
        .arg("regenerate")
        .arg("--check")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();
}

#[test]
fn regenerate_check_fails_on_drift() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .build()
        .expect("fixture");
    write_root_templates(fixture.path());

    mock()
        .arg("regenerate")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();

    // Hand-edit one rendered file to simulate stale committed copy.
    fs::write(fixture.path().join("docs").join("PRINCIPLES.md"), "stale").unwrap();

    mock()
        .arg("regenerate")
        .arg("--check")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .failure();
}

#[test]
fn regenerate_check_fails_when_output_missing() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .build()
        .expect("fixture");
    write_root_templates(fixture.path());

    // No regenerate first: docs/ is absent.
    mock()
        .arg("regenerate")
        .arg("--check")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .failure();
}

#[test]
fn regenerate_honours_out_dir_override() {
    let fixture = MockspaceFixture::new()
        .with_mock_dir()
        .build()
        .expect("fixture");
    write_root_templates(fixture.path());

    let scratch = fixture.path().join("scratch_docs");
    mock()
        .arg("regenerate")
        .arg("--out-dir")
        .arg(&scratch)
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();

    assert!(scratch.join("DESIGN.md").is_file());
    assert!(
        !fixture.path().join("docs").exists(),
        "default docs/ should not be created when --out-dir overrides"
    );
}
