//! Golden-result end-to-end tests for `cargo mock install` that
//! snapshot the resulting filesystem tree.
//!
//! Pattern: invoke the CLI against a fresh fixture, walk the
//! resulting directory tree, serialise to a deterministic text
//! representation, and compare against a checked-in golden.
//! Catches structural drift in what bootstrap writes to disk:
//! file paths, contents, and (on Unix) the executable bit.
//!
//! This complements `e2e_explain.rs` which captures stdout-only
//! goldens. The tree-walk variant catches a different class of
//! drift: a refactor that changes which files install creates,
//! or where, would land here.

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;
use std::fs;
use std::path::Path;

mod common;
use common::assert_matches_golden;

/// Walk the directory rooted at `root` and produce a deterministic
/// text snapshot. Output shape per file:
///
/// ```text
/// <relative-path>[ (executable)]
/// +++ content +++
/// <file body>
/// --- end ---
/// ```
///
/// Files are emitted in sorted path order so the snapshot stays
/// stable across runs. Directories surface only via their contained
/// files (an empty directory is invisible). The executable annotation
/// fires on Unix when the user-execute bit is set; on non-Unix the
/// annotation is omitted.
fn snapshot_tree(root: &Path) -> String {
    let mut entries: Vec<(String, std::path::PathBuf)> = Vec::new();
    collect(root, root, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    for (rel, abs) in &entries {
        out.push_str(rel);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(abs) {
                if meta.permissions().mode() & 0o100 != 0 {
                    out.push_str(" (executable)");
                }
            }
        }
        out.push('\n');
        out.push_str("+++ content +++\n");
        let body = match fs::read_to_string(abs) {
            Ok(body) => body,
            Err(_) => String::from("<non-utf8 binary content>\n"),
        };
        out.push_str(&body);
        // Guard against the body lacking a trailing newline so the
        // `--- end ---` delimiter is always on its own line. Checking
        // the body (not `out`) is the correct test: after the first
        // file `out` always ends in `--- end ---\n` regardless of
        // whether the next body ends in a newline.
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("--- end ---\n");
    }
    out
}

/// Recursively collect (relative-path-as-forward-slash, absolute-path)
/// pairs under `current` rooted at `root`. Hidden directories like
/// `.git` are walked through; their contents matter for install
/// snapshots (e.g. `.git/config` for `core.hooksPath`).
fn collect(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .expect("entries are under root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
}

fn mock() -> Command {
    Command::cargo_bin("mock").expect("cargo build provides the mock binary")
}

// ---- install on bare fixture ---------------------------------------------

#[test]
fn install_on_bare_fixture_produces_canonical_tree() {
    // Bare fixture (no pre-existing files), then `mock install`.
    // The golden captures every file bootstrap creates: cargo
    // alias config, two hook scripts, and the `.git/config` with
    // `core.hooksPath` pointing at `mock/target/hooks`. Any change
    // to bootstrap's filesystem footprint flags here.
    let fixture = MockspaceFixture::new().build().expect("fixture");
    mock()
        .arg("install")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();
    let snapshot = snapshot_tree(fixture.path());
    assert_matches_golden("install_bare_fixture_tree", &snapshot);
}
