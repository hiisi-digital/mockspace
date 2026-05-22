//! Golden-result end-to-end tests for `cargo mock phase`.
//!
//! Phase verbs operate on existing round refs (`refs/mock/round/<slug>`).
//! There is no CLI verb for creating a round in TOPIC phase today;
//! that initial state is set up via direct library access through
//! [`RepoHandle::write_round_ref`] before the CLI invocation runs.
//! That seeding pattern matches the unit-test idiom in
//! `mockspace-core::io::advance::tests`.
//!
//! Each test sets up a topic-phase round, runs one or more phase
//! verbs via the CLI, and snapshots stdout against a checked-in
//! golden. The commit OID in the output is non-deterministic (the
//! committer signature carries a wall-clock timestamp) and gets
//! scrubbed before comparison.

use assert_cmd::Command;
use mockspace_rs::{RefPath, RepoHandle, RoundRefTree, Slug};
use mockspace_test_fixtures::MockspaceFixture;
use std::collections::BTreeMap;
use std::process::Command as StdCommand;

mod common;
use common::assert_matches_golden;

/// `git init --quiet` inside the fixture so phase verbs have a repo
/// to write refs into.
fn git_init(fixture: &MockspaceFixture) {
    let status = StdCommand::new("git")
        .args(["init", "--quiet"])
        .current_dir(fixture.path())
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

/// Write a round ref carrying just the `.phase` marker file. Mirrors
/// the `seed_round` helper used inside `mockspace-core::io::advance`
/// unit tests; lifted here because the e2e test crate cannot reach
/// into another crate's `#[cfg(test)]` module.
fn seed_topic_round(fixture: &MockspaceFixture, slug: &Slug) {
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let ref_path = RefPath::round_mock(slug);
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(".phase".to_owned(), b"topic\n".to_vec());
    let tree = RoundRefTree::from_entries(entries);
    handle
        .write_round_ref(&ref_path, &tree, "seed topic round", None)
        .expect("seed round ref");
}

/// Invoke `cargo mock phase <args>` against the fixture and capture
/// stdout. Panics on non-zero exit.
fn run_phase(fixture: &MockspaceFixture, args: &[&str]) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .arg("phase")
        .args(args)
        .output()
        .expect("invoke mock phase");
    assert!(
        output.status.success(),
        "mock phase {args:?} exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("phase stdout is UTF-8")
}

/// Replace lines whose `trim_start()` begins with `new commit:` with
/// a stable placeholder. The commit signature carries a wall-clock
/// timestamp so the OID varies per run.
fn scrub_commit_oid(s: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("new commit:") {
                let indent = &line[..line.len() - trimmed.len()];
                format!("{indent}new commit: <OID>")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn plan_verb_advances_topic_to_plan_doc() {
    // Setup: bare fixture + git init + topic-phase round seeded
    // directly via the library. The CLI provides no verb for
    // creating a topic round (rounds enter the system pre-existing
    // in v2 design); seeding via RepoHandle is the contract-aware
    // way to set up this initial state from a test.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let slug = Slug::new("arvo-graph").expect("slug");
    seed_topic_round(&fixture, &slug);

    // Act: run the plan verb. TOPIC -> PLAN(doc).
    let stdout = run_phase(&fixture, &["plan", slug.as_str()]);

    // Assert: stdout shape matches golden after OID scrub.
    let scrubbed = scrub_commit_oid(&stdout);
    assert_matches_golden("phase_plan_advances_topic_to_plan_doc", &scrubbed);

    // Integrity: the on-disk `.phase` marker is now plan_doc.
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let tree = handle
        .read_ref_tree(&RefPath::round_mock(&slug))
        .expect("read ref tree");
    let phase_bytes = tree
        .get(".phase")
        .expect("`.phase` entry present after plan");
    assert_eq!(
        phase_bytes,
        b"plan_doc\n",
        "expected `.phase` to contain `plan_doc\\n`; got {:?}",
        std::str::from_utf8(phase_bytes).unwrap_or("<non-utf8>")
    );
}

#[test]
fn plan_verb_refuses_when_round_missing() {
    // No round seeded; the plan verb must reject with a typed
    // "round ref not found" error rather than crash or create.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "plan", "missing-round"])
        .output()
        .expect("invoke mock phase plan");
    assert!(
        !output.status.success(),
        "phase plan against missing round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("phase transition failed"),
        "expected typed phase-failure prefix in stderr; got: {stderr}"
    );
}
