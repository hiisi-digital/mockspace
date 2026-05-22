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
use mockspace_rs::{
    AcceptanceBlock, ChangeBlock, Manifest, ManifestSide, RefPath, RepoHandle, RoundRefTree,
    ScopeBlock, Slug, VerifierCheck, VerifierKind,
};
use mockspace_test_fixtures::MockspaceFixture;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command as StdCommand;

mod common;
use common::{assert_matches_golden, scrub_oid_lines};

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
    let scrubbed = scrub_oid_lines(&stdout, "new commit:");
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

#[test]
fn apply_verb_rejects_invalid_source_tip_hex() {
    // The --source-tip flag uses value_parser = parse_object_id; an
    // invalid hex string must fail at clap parse time before any IO
    // happens. Pins the typed-clap rejection from the #591 audit.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "apply", "some-round", "--source-tip", "not-a-hex"])
        .output()
        .expect("invoke mock phase apply");
    assert!(
        !output.status.success(),
        "phase apply with invalid hex must reject at clap parse; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("invalid object id"),
        "expected clap-time typed-parse rejection naming object-id error; got: {stderr}"
    );
}

#[test]
fn apply_verb_refuses_when_round_missing() {
    // No round seeded; apply must reject typed phase-failure even
    // with a valid (but unused) source-tip hex.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let zero_oid = "0000000000000000000000000000000000000000";

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "apply", "missing-round", "--source-tip", zero_oid])
        .output()
        .expect("invoke mock phase apply");
    assert!(
        !output.status.success(),
        "phase apply against missing round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("phase transition failed"),
        "expected typed phase-failure prefix; got: {stderr}"
    );
}

#[test]
fn finish_verb_refuses_when_round_missing() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "finish", "missing-round"])
        .output()
        .expect("invoke mock phase finish");
    assert!(
        !output.status.success(),
        "phase finish against missing round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("phase transition failed"),
        "expected typed phase-failure prefix; got: {stderr}"
    );
}

#[test]
fn replan_verb_refuses_when_round_missing() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "replan", "missing-round"])
        .output()
        .expect("invoke mock phase replan");
    assert!(
        !output.status.success(),
        "phase replan against missing round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("phase transition failed"),
        "expected typed phase-failure prefix; got: {stderr}"
    );
}

#[test]
fn plan_verb_refuses_when_round_already_advanced() {
    // Seed a round already in PLAN(doc); plan against a non-TOPIC
    // round must reject with a typed phase-failure. Catches the
    // transition validity matrix's TOPIC -> PLAN(doc) precondition.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let slug = Slug::new("already-planned").expect("slug");
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
    let tree = RoundRefTree::from_entries(entries);
    handle
        .write_round_ref(&RefPath::round_mock(&slug), &tree, "seed", None)
        .expect("seed");

    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "plan", slug.as_str()])
        .output()
        .expect("invoke mock phase plan");
    assert!(
        !output.status.success(),
        "plan against plan_doc round must reject; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("phase transition failed"),
        "expected typed phase-failure prefix; got: {stderr}"
    );
}

/// Create a real git commit in the fixture so phase apply can resolve a
/// source-tip OID. Writes a `README.md`, stages it, commits, then parses
/// the resulting HEAD OID via `git rev-parse HEAD`.
fn init_source_tip(fixture: &MockspaceFixture, filename: &str, content: &str) -> String {
    std::fs::write(fixture.path().join(filename), content).expect("write file");
    let add = StdCommand::new("git")
        .args(["add", filename])
        .current_dir(fixture.path())
        .status()
        .expect("git add runs");
    assert!(add.success(), "git add failed");
    let commit_status = StdCommand::new("git")
        .args(["-c", "user.email=t@t.local", "-c", "user.name=tester", "commit", "--quiet", "-m", "source tip"])
        .current_dir(fixture.path())
        .status()
        .expect("git commit runs");
    assert!(commit_status.success(), "git commit failed");
    let out = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.path())
        .output()
        .expect("git rev-parse runs");
    assert!(out.status.success(), "git rev-parse failed");
    String::from_utf8(out.stdout).expect("rev-parse stdout is UTF-8").trim().to_owned()
}

/// Construct a valid minimal doc-side manifest for `slug` and serialise
/// to TOML. Mirrors the canonical fixture in mockspace-core::io::advance
/// unit tests; lifted here because the cli e2e crate cannot reach into
/// another crate's `#[cfg(test)]` module.
fn doc_manifest_toml(slug: &str) -> String {
    Manifest {
        mockspace_version: "1.0".to_owned(),
        round_slug: slug.to_owned(),
        phase: ManifestSide::Doc,
        scope: ScopeBlock {
            description: "test apply happy path".to_owned(),
            in_scope_tasks: vec![],
            out_of_scope: vec![],
        },
        acceptance: AcceptanceBlock {
            criteria: "passes".to_owned(),
        },
        changes: vec![ChangeBlock {
            task: None,
            file: PathBuf::from("README.md"),
            description: "doc change".to_owned(),
            verify: VerifierCheck::Kind(VerifierKind::PathExists {
                file: PathBuf::from("README.md"),
            }),
        }],
        deprecated_accounting: vec![],
    }
    .to_toml()
    .expect("manifest toml")
}

#[test]
fn apply_verb_seals_doc_manifest_and_advances_to_apply_doc() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let source_tip = init_source_tip(&fixture, "README.md", "hello\n");
    let slug = Slug::new("apply-happy").expect("slug");

    // Seed a PLAN(doc) round with the matching manifest blob.
    let handle = RepoHandle::open(fixture.path()).expect("open repo");
    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
    entries.insert(
        "manifest.doc.toml".to_owned(),
        doc_manifest_toml(slug.as_str()).into_bytes(),
    );
    let tree = RoundRefTree::from_entries(entries);
    handle
        .write_round_ref(&RefPath::round_mock(&slug), &tree, "seed plan-doc round", None)
        .expect("seed round ref");

    // Act: mock phase apply --source-tip <oid>.
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["phase", "apply", slug.as_str(), "--source-tip", &source_tip])
        .output()
        .expect("invoke mock phase apply");
    assert!(
        output.status.success(),
        "phase apply exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("apply stdout is UTF-8");
    assert!(
        stdout.contains("phase `apply_doc`"),
        "expected stdout to name apply_doc landing phase; got: {stdout}"
    );
    assert!(
        stdout.contains("locked manifest"),
        "expected stdout to report locked manifest; got: {stdout}"
    );

    // Integrity: the on-disk `.phase` marker is now apply_doc and the
    // doc manifest is locked under `manifest.doc.lock.toml`.
    let post = handle
        .read_ref_tree(&RefPath::round_mock(&slug))
        .expect("read ref tree");
    let phase_bytes = post.get(".phase").expect(".phase present after apply");
    assert_eq!(
        phase_bytes,
        b"apply_doc\n",
        "expected .phase to advance to apply_doc; got {:?}",
        std::str::from_utf8(phase_bytes).unwrap_or("<non-utf8>")
    );
    assert!(
        post.get("manifest.doc.locked.toml").is_some(),
        "expected locked doc manifest at manifest.doc.locked.toml"
    );
}

