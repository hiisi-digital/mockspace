//! Golden-result end-to-end tests for `cargo mock task`.
//!
//! Walks the full task lifecycle as a user would: new, list, show,
//! start, block, defer, close, move. Each verb's stdout is captured
//! and the concatenated transcript is golden-snapshotted. Catches
//! drift in:
//!
//! - The per-verb output shape (header lines, ref + commit fields).
//! - The on-disk meta.toml rendering (via `show` after each
//!   transition).
//! - The list ordering after a move re-namespaces a task.
//!
//! Two nondeterministic surfaces get scrubbed before comparison:
//! commit OIDs (vary with timestamps in the committer signature)
//! and the `created` timestamp inside `meta.toml`. Both collapse to
//! stable placeholders.

use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;

mod common;
use common::{assert_matches_golden, scrub_oid_lines};

/// Run `git init --quiet` inside the fixture so task refs (which
/// live on `refs/mock/task/...`) have somewhere to land. Task verbs
/// refuse if `.git/` is missing.
fn git_init(fixture: &MockspaceFixture) {
    let status = StdCommand::new("git")
        .args(["init", "--quiet"])
        .current_dir(fixture.path())
        .status()
        .expect("git init runs");
    assert!(status.success(), "git init failed");
}

/// Invoke `cargo mock task <args>` against the fixture and capture
/// stdout. Panics on non-zero exit so the test surfaces the failing
/// invocation immediately instead of comparing against a stale
/// golden.
fn run_task(fixture: &MockspaceFixture, args: &[&str]) -> String {
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .arg("task")
        .args(args)
        .output()
        .expect("invoke mock task");
    assert!(
        output.status.success(),
        "mock task {args:?} exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("task stdout is UTF-8")
}

/// Replace the `created = "<ISO-8601>"` line inside a serialised
/// TaskMeta TOML with a stable placeholder. Lives here (not in the
/// shared `common` module) because it's TaskMeta-specific; the
/// generic `scrub_oid_lines` covers the OID-line case.
fn scrub_created_timestamp(s: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("created = \"") {
                let indent = &line[.. line.len() - trimmed.len()];
                format!("{indent}created = \"<TIMESTAMP>\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Apply both scrubs in sequence: OID lines first, then the
/// TaskMeta `created` timestamp.
fn scrub(s: &str) -> String {
    scrub_created_timestamp(&scrub_oid_lines(s, "commit:"))
}

#[test]
fn task_lifecycle_end_to_end() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    let mut transcript = String::new();

    transcript.push_str("=== task new (top-level) ===\n");
    transcript.push_str(&run_task(&fixture, &[
        "new",
        "migrate-to-codeberg",
        "--title",
        "Migrate to Codeberg",
    ]));

    transcript.push_str("\n=== task new (namespaced) ===\n");
    transcript.push_str(&run_task(&fixture, &[
        "new",
        "compiler::ir::lower-pass",
        "--title",
        "Lower pass for IR",
    ]));

    transcript.push_str("\n=== task list ===\n");
    transcript.push_str(&run_task(&fixture, &["list"]));

    transcript.push_str("\n=== task show (top-level) ===\n");
    transcript.push_str(&run_task(&fixture, &["show", "migrate-to-codeberg"]));

    transcript.push_str("\n=== task start ===\n");
    transcript.push_str(&run_task(&fixture, &["start", "migrate-to-codeberg"]));

    transcript.push_str("\n=== task block ===\n");
    transcript.push_str(&run_task(&fixture, &["block", "migrate-to-codeberg"]));

    transcript.push_str("\n=== task defer ===\n");
    transcript.push_str(&run_task(&fixture, &["defer", "migrate-to-codeberg"]));

    transcript.push_str("\n=== task close ===\n");
    transcript.push_str(&run_task(&fixture, &[
        "close",
        "migrate-to-codeberg",
        "--resolution",
        "completed",
        "--branch",
        "feat/codeberg",
    ]));

    transcript.push_str("\n=== task move ===\n");
    transcript.push_str(&run_task(&fixture, &[
        "move",
        "compiler::ir::lower-pass",
        "compiler::backend::lower-pass",
    ]));

    transcript.push_str("\n=== task list (after move) ===\n");
    transcript.push_str(&run_task(&fixture, &["list"]));

    let scrubbed = scrub(&transcript);
    assert_matches_golden("task_lifecycle_end_to_end", &scrubbed);
}

#[test]
fn task_rejects_invalid_slug_in_id() {
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    // Uppercase fails slug validation at clap parse time, before any
    // git operation runs. The CLI exits non-zero with the typed
    // SlugError in the message.
    let output = Command::cargo_bin("mock")
        .expect("cargo build provides the mock binary")
        .arg("--repo-root")
        .arg(fixture.path())
        .args(["task", "new", "BadCase", "--title", "x"])
        .output()
        .expect("invoke mock task");
    assert!(
        !output.status.success(),
        "task new with uppercase slug must reject; stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("invalid task id") && stderr.contains("not a valid slug"),
        "expected typed clap-parse rejection naming the slug error, got: {stderr}"
    );
}

#[test]
fn task_show_after_move_reflects_new_namespace() {
    // Verifies the move's meta.toml rewrite: after `task move
    // src::leaf -> dst::leaf`, `task show dst::leaf` reports the
    // new namespace path. This is the integrity guard that catches
    // a regression where move only renames the ref but leaves the
    // meta.toml namespace stale.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    git_init(&fixture);

    run_task(&fixture, &["new", "alpha::beta::leaf", "--title", "x"]);
    run_task(&fixture, &["move", "alpha::beta::leaf", "gamma::leaf"]);

    let show = run_task(&fixture, &["show", "gamma::leaf"]);
    let scrubbed = scrub_created_timestamp(&show);
    let lines: Vec<&str> = scrubbed.lines().collect();
    assert!(
        lines.iter().any(|l| l.trim() == "slug = \"leaf\""),
        "show output missing slug = \"leaf\" line; got:\n{scrubbed}"
    );
    assert!(
        lines.iter().any(|l| l.trim() == "namespace = \"gamma\""),
        "show output missing namespace = \"gamma\" line; got:\n{scrubbed}"
    );
}

/// Marker reference for the `fixture.path()` accessor used above.
/// Kept here so a future build that re-routes the fixture's home
/// dir surfaces the dependency loudly.
#[allow(dead_code)]
fn _fixture_path_compiles(fixture: &MockspaceFixture) -> &Path {
    fixture.path()
}
