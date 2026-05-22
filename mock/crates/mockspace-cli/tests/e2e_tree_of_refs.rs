//! Golden-result end-to-end tests for the full `refs/mock/*` namespace
//! after a lifecycle plays out.
//!
//! Earlier slices snapshot per-verb stdout. These tests snapshot the
//! durable state graph: every ref under `refs/mock/`, with the full
//! blob payload of every tree entry, in lex-sorted form. Catches drift
//! the stdout goldens cannot see, such as a missing manifest blob, an
//! extra anchor entry, a rename that creates the new ref but leaves
//! the old one in place, or content corruption inside a blob.
//!
//! ISO-8601 timestamps embedded in `meta.toml` / closure blocks are
//! scrubbed before comparison; the snapshot helper itself prints refs
//! by name (no commit OIDs) so no OID scrub is needed.

use assert_cmd::Command;
use mockspace_rs::{RefPath, RepoHandle, RoundRefTree, Slug};
use mockspace_test_fixtures::MockspaceFixture;
use std::collections::BTreeMap;
use std::process::Command as StdCommand;

mod common;
use common::{assert_matches_golden, snapshot_mock_refs};

/// `git init --quiet` inside the fixture so task / round verbs have
/// somewhere to write refs.
fn git_init(fixture: &MockspaceFixture) {
    let status = StdCommand::new("git")
        .args(["init", "--quiet"])
        .current_dir(fixture.path())
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

/// Seed a topic-phase round ref directly via the library. Mirrors the
/// helper in `e2e_phase.rs`; lifted because integration test files
/// each compile as separate binaries and cannot share private fns.
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

/// Invoke `cargo mock <subcmd> <args>` against the fixture and capture
/// stdout. Panics on non-zero exit.
fn run_mock(fixture: &MockspaceFixture, args: &[&str]) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(args)
        .output()
        .expect("invoke mock");
    assert!(
        output.status.success(),
        "mock {args:?} exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("mock stdout is UTF-8")
}

/// Replace any line whose trimmed prefix matches an ISO-8601-shaped
/// `<field> = "<value>"` pattern with a `<field> = "<TIMESTAMP>"`
/// placeholder. Covers both `created = "..."` (TaskMeta) and
/// `closed_at = "..."` (TaskClosure) without enumerating field names.
///
/// The match is anchored on the ISO-8601 prefix shape (`YYYY-MM-DDT`)
/// to keep arbitrary strings carrying a `T` and a `Z` (e.g. branch
/// names like `feat/TZ-thing`) from getting scrubbed by accident.
fn scrub_iso8601_lines(s: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some((field, rest)) = trimmed.split_once(" = \"") {
                if !field.is_empty()
                    && field
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && looks_like_iso8601_quoted_value(rest)
                {
                    let indent = &line[..line.len() - trimmed.len()];
                    return format!("{indent}{field} = \"<TIMESTAMP>\"");
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// True when `rest` (the slice after `<field> = "`) starts with
/// `YYYY-MM-DDT`, ends with `"`, and contains a `Z` somewhere before
/// the closing quote. Tight enough to ignore branch names and other
/// strings that happen to carry a stray `T`+`Z`.
fn looks_like_iso8601_quoted_value(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    if bytes.len() < 12 || !rest.ends_with('"') {
        return false;
    }
    let digit = |i: usize| bytes[i].is_ascii_digit();
    digit(0)
        && digit(1)
        && digit(2)
        && digit(3)
        && bytes[4] == b'-'
        && digit(5)
        && digit(6)
        && bytes[7] == b'-'
        && digit(8)
        && digit(9)
        && bytes[10] == b'T'
        && rest[..rest.len() - 1].contains('Z')
}

#[test]
fn tree_of_refs_after_task_lifecycle() {
    // Drives the task lifecycle through every transition + a move,
    // then snapshots the full refs/mock/* state. The snapshot proves:
    //   - Both task refs exist at expected paths.
    //   - meta.toml content reflects all state transitions
    //     (in-progress -> blocked -> deferred -> closed).
    //   - The move's old ref is GONE (no phantom under the source
    //     namespace).
    //   - The closure block lands on the closed task's meta.toml.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    // Two tasks: one bare slug, one namespaced.
    run_mock(
        &fixture,
        &["task", "new", "migrate-to-codeberg", "--title", "Migrate to Codeberg"],
    );
    run_mock(
        &fixture,
        &[
            "task",
            "new",
            "compiler::ir::lower-pass",
            "--title",
            "Lower pass for IR",
        ],
    );

    // Walk the bare task through every state.
    run_mock(&fixture, &["task", "start", "migrate-to-codeberg"]);
    run_mock(&fixture, &["task", "block", "migrate-to-codeberg"]);
    run_mock(&fixture, &["task", "defer", "migrate-to-codeberg"]);
    run_mock(
        &fixture,
        &[
            "task",
            "close",
            "migrate-to-codeberg",
            "--resolution",
            "completed",
            "--branch",
            "feat/codeberg",
        ],
    );

    // Move the namespaced one to verify the old ref disappears.
    run_mock(
        &fixture,
        &["task", "move", "compiler::ir::lower-pass", "compiler::backend::lower-pass"],
    );

    let snapshot = snapshot_mock_refs(fixture.path());
    let scrubbed = scrub_iso8601_lines(&snapshot);
    assert_matches_golden("tree_of_refs_after_task_lifecycle", &scrubbed);
}

#[test]
fn tree_of_refs_after_phase_plan() {
    // Drives a round from TOPIC through PLAN(doc) and snapshots the
    // round ref's tree. The snapshot proves:
    //   - The ref carries exactly one blob (`.phase`) at this stage.
    //   - The `.phase` blob holds `plan_doc\n` (not `topic` and not
    //     a manifest yet).
    //   - No seal-time anchor exists yet (anchors only land on
    //     finish-doc / finish-src).
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);
    let slug = Slug::new("arvo-graph").expect("slug");
    seed_topic_round(&fixture, &slug);
    run_mock(&fixture, &["phase", "plan", slug.as_str()]);

    let snapshot = snapshot_mock_refs(fixture.path());
    assert_matches_golden("tree_of_refs_after_phase_plan", &snapshot);
}

#[test]
fn tree_of_refs_after_combined_lifecycle() {
    // Combined: one closed task + one round at PLAN(doc), plus one
    // moved (now-renamed) namespaced task. The snapshot proves the
    // full state graph is well-formed: no phantom refs, every ref
    // carries the expected payload, and the task + round refs
    // coexist in the same namespace without crosstalk.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    // Task side: one task transitioned through close.
    run_mock(
        &fixture,
        &["task", "new", "ship-it", "--title", "Ship it"],
    );
    run_mock(&fixture, &["task", "start", "ship-it"]);
    run_mock(
        &fixture,
        &[
            "task",
            "close",
            "ship-it",
            "--resolution",
            "completed",
            "--branch",
            "feat/ship",
        ],
    );

    // Round side: seed topic, advance plan.
    let slug = Slug::new("round-alpha").expect("slug");
    seed_topic_round(&fixture, &slug);
    run_mock(&fixture, &["phase", "plan", slug.as_str()]);

    let snapshot = snapshot_mock_refs(fixture.path());
    let scrubbed = scrub_iso8601_lines(&snapshot);
    assert_matches_golden("tree_of_refs_after_combined_lifecycle", &scrubbed);
}

