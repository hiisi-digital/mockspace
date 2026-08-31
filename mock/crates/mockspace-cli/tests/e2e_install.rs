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

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use mockspace_test_fixtures::MockspaceFixture;

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
            // Stat must succeed under a fixture-controlled tempdir.
            // A failure here means the snapshot is silently incomplete,
            // which makes the golden test pass against an incorrect
            // tree shape. Panic loud instead.
            let meta = fs::metadata(abs)
                .unwrap_or_else(|e| panic!("metadata read failed for {abs:?}: {e}"));
            if meta.permissions().mode() & 0o100 != 0 {
                out.push_str(" (executable)");
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
fn collect(root: &Path, current: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    // Same rationale as the metadata stat above: a fixture-controlled
    // tempdir that fails to read_dir is a snapshot bug, not a permitted
    // run-time condition. Panic loud so a missing subdir doesn't
    // silently shrink the snapshot.
    let entries =
        fs::read_dir(current).unwrap_or_else(|e| panic!("read_dir failed for {current:?}: {e}"));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("read_dir entry failed under {current:?}: {e}"));
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

// ---- uninstall round trip ------------------------------------------------

#[test]
fn install_then_uninstall_produces_residue_tree() {
    // Install then uninstall via the CLI. The golden captures
    // what bootstrap leaves behind after teardown. The current
    // contract: hook script files removed; cargo alias entry and
    // `core.hooksPath` cleared from their respective config files;
    // the config files themselves remain on disk (potentially
    // empty or sparse) because removing a config the user may
    // have authored is overreach. Directory entries like
    // `mock/target/hooks/` may persist as empty dirs and are
    // invisible to the snapshot.
    let fixture = MockspaceFixture::new()
        .with_install()
        .build()
        .expect("fixture");
    mock()
        .arg("uninstall")
        .arg("--repo-root")
        .arg(fixture.path())
        .assert()
        .success();
    let snapshot = snapshot_tree(fixture.path());
    assert_matches_golden("install_then_uninstall_residue_tree", &snapshot);
}
