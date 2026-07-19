//! Shared helpers for the e2e golden-result test files.
//!
//! Integration test files under `tests/` each compile as a separate
//! cargo test binary, so shared code lives in a `common/mod.rs`
//! submodule that each file includes via `mod common;`. This is the
//! idiomatic Rust pattern.
//!
//! See `e2e_explain.rs` for the inaugural use and `e2e_install.rs`
//! for the filesystem-tree variant.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

/// Resolve the path to a checked-in golden file under
/// `<crate>/tests/goldens/<name>.golden`. Centralised so consumers
/// don't repeat the `CARGO_MANIFEST_DIR` join idiom.
pub fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join(format!("{name}.golden"))
}

/// Replace lines whose `trim_start()` begins with `prefix` with a
/// stable `<prefix> <OID>` placeholder. Used by e2e tests to scrub
/// the per-verb OID output lines (whose values vary per run because
/// the committer signature carries a wall-clock timestamp) before
/// golden comparison.
///
/// The `prefix` must include the trailing colon and any leading
/// space inside the line, e.g. `"commit:"`, `"new commit:"`,
/// `"new archive commit:"`. The leading indentation is preserved
/// verbatim from the input line.
///
/// Output always ends with a single trailing newline, matching the
/// per-call-site idiom that existed before consolidation.
#[allow(dead_code)]
pub fn scrub_oid_lines(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with(prefix) {
                let indent = &line[.. line.len() - trimmed.len()];
                format!("{indent}{prefix} <OID>")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Snapshot every ref under `refs/mock/` plus the full blob payload
/// of each ref's tree, formatted as a stable sorted text blob suitable
/// for golden comparison.
///
/// The output shape is one block per ref, sorted by refname. Each
/// block looks like:
///
/// ```text
/// === <refname> ===
/// <path-1>:
/// <blob-1 content>
///
/// <path-2>:
/// <blob-2 content>
/// ```
///
/// Blob content is reproduced verbatim. Callers that need to scrub
/// non-deterministic fields (timestamps, commit OIDs embedded in
/// payloads) apply [`scrub_oid_lines`] / per-test scrubbers AFTER
/// snapshotting.
///
/// Implementation uses git plumbing (`for-each-ref` + `ls-tree -r` +
/// `cat-file -p`) rather than the in-process [`RoundRefTree`] reader
/// so the snapshot reflects literal on-disk state, including any
/// drift between the writer's intent and what landed.
///
/// Panics on git invocation failure; e2e tests treat that as a hard
/// test infrastructure error, not a snapshot drift.
#[allow(dead_code)]
pub fn snapshot_mock_refs(repo_root: &Path) -> String {
    let listing = git_capture(repo_root, &[
        "for-each-ref",
        "refs/mock",
        "--format=%(refname)",
        "--sort=refname",
    ]);
    let mut out = String::new();
    for refname in listing.lines() {
        if refname.is_empty() {
            continue;
        }
        out.push_str("=== ");
        out.push_str(refname);
        out.push_str(" ===\n");
        let paths = git_capture(repo_root, &["ls-tree", "-r", "--name-only", refname]);
        for path in paths.lines() {
            if path.is_empty() {
                continue;
            }
            out.push_str(path);
            out.push_str(":\n");
            let spec = format!("{refname}:{path}");
            let content = git_capture(repo_root, &["cat-file", "-p", &spec]);
            out.push_str(&content);
            if !content.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
    }
    out
}

fn git_capture(repo_root: &Path, args: &[&str]) -> String {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|e| panic!("invoke git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} exited non-zero; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout is UTF-8")
}

/// Compare `actual` against the checked-in golden at `<name>.golden`.
/// On `MOCKSPACE_UPDATE_GOLDENS=1`, writes `actual` to the golden
/// path (creating the parent directory if needed) and passes.
/// Otherwise reads the golden and asserts byte-equality, failing
/// with a diff hint pointing at the regenerate knob.
pub fn assert_matches_golden(name: &str, actual: &str) {
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
        Err(e) => {
            panic!(
                "golden `{name}` missing at {}: {e}.\n\
             To create it, rerun with MOCKSPACE_UPDATE_GOLDENS=1",
                path.display()
            )
        },
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
