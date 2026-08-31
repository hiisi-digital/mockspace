//! Golden-result end-to-end tests for `cargo mock close`.
//!
//! `close` archives a DONE-phase round: it reads the round ref tree,
//! splices the entries under `<slug>/` into `refs/mock/round-archive`,
//! then deletes the source `refs/mock/round/<slug>`. Tests here pin
//! the stdout shape and verify the resulting ref-tree topology.
//!
//! The DONE-phase initial state is seeded directly via
//! [`RepoHandle::write_round_ref`]; no CLI verb advances a round to
//! DONE from a bare fixture (that requires the full phase sequence
//! with seeded manifests + source-tip, which is its own future test
//! slice).

use std::collections::BTreeMap;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use mockspace_rs::{RefPath, RefTreeReadError, RepoHandle, RoundRefTree, Slug};
use mockspace_test_fixtures::MockspaceFixture;

mod common;
use common::{assert_matches_golden, scrub_oid_lines};

/// `git init --quiet` inside the fixture so the close verb has a
/// repo to read refs from.
fn git_init(fixture: &MockspaceFixture) {
    let status = StdCommand::new("git")
        .args(["init", "--quiet"])
        .current_dir(fixture.path())
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

/// Seed a round ref in DONE phase. The body carries just the
/// `.phase` marker; downstream consumers archive whatever entries
/// the round held, so a minimal one-entry seed is the cleanest
/// shape for stdout assertions about `entries archived: N`.
fn seed_done_round(fixture: &MockspaceFixture, slug: &Slug) {
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let ref_path = RefPath::round_mock(slug);
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(".phase".to_owned(), b"done\n".to_vec());
    let tree = RoundRefTree::from_entries(entries);
    handle
        .write_round_ref(&ref_path, &tree, "seed done round", None)
        .expect("seed round ref");
}

/// Invoke `cargo mock close <slug>` and capture stdout. Panics on
/// non-zero exit.
fn run_close(fixture: &MockspaceFixture, slug: &str) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["close", slug])
        .output()
        .expect("invoke mock close");
    assert!(
        output.status.success(),
        "mock close {slug} exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("close stdout is UTF-8")
}

#[test]
fn close_archives_done_round_and_deletes_source() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let slug = Slug::new("arvo-graph-csr").expect("slug");
    seed_done_round(&fixture, &slug);

    let stdout = run_close(&fixture, slug.as_str());
    let scrubbed = scrub_oid_lines(&stdout, "new archive commit:");
    assert_matches_golden("close_archives_done_round", &scrubbed);

    // Integrity 1: source ref gone.
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    assert!(
        matches!(
            handle.resolve_ref_oid(&RefPath::round_mock(&slug)),
            Err(RefTreeReadError::RefNotFound { .. })
        ),
        "expected source round ref to be deleted after close"
    );

    // Integrity 2: archive ref present and carries the slug-prefixed
    // entry.
    let archive_tree = handle
        .read_ref_tree(&RefPath::round_archive())
        .expect("read archive ref");
    let expected_key = format!("{}/.phase", slug.as_str());
    let phase_bytes = archive_tree
        .get(&expected_key)
        .unwrap_or_else(|| panic!("expected `{expected_key}` in archive tree"));
    assert_eq!(
        phase_bytes, b"done\n",
        "archived `.phase` entry must preserve the round's DONE marker"
    );
}

#[test]
fn close_refuses_when_round_not_done() {
    // Seed a topic-phase round; close must refuse with the typed
    // NotDone error since archive requires DONE.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let slug = Slug::new("not-done").expect("slug");
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(".phase".to_owned(), b"topic\n".to_vec());
    let tree = RoundRefTree::from_entries(entries);
    handle
        .write_round_ref(&RefPath::round_mock(&slug), &tree, "seed topic", None)
        .expect("seed");

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["close", slug.as_str()])
        .output()
        .expect("invoke mock close");
    assert!(
        !output.status.success(),
        "close against non-DONE round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("archive failed"),
        "expected typed archive-failure prefix; got: {stderr}"
    );
}

#[test]
fn close_refuses_when_round_missing() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["close", "never-existed"])
        .output()
        .expect("invoke mock close");
    assert!(
        !output.status.success(),
        "close on missing round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("archive failed"),
        "expected typed archive-failure prefix; got: {stderr}"
    );
}
