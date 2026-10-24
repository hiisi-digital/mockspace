//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The lockfile the bench build writes beside the tree is output of the run,
/// and the run must not move the digest it recorded.
#[test]
fn a_lockfile_written_under_the_benches_does_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/Cargo.lock"), "# written by the build")
    }));
}

// ── the lockfile: the build's own is output, a committed one is the consumer's ──

fn git(root: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A committed lockfile beside an arm is how a consumer picks the framework
/// commit the arms are built at (`bench_rev`), so changing it changes the
/// tree. Only the one the build writes, untracked, is output.
#[test]
fn a_committed_lockfile_under_the_benches_moves_the_digest() {
    for rel in ["benches/Cargo.lock", "benches/sample/arms/a/Cargo.lock"] {
        let (_t, root, mock) = repo();
        write(&mock.join(rel), "# rev one\n");
        git(&root, &["add", "-A"]);
        let before = now(&root, &mock);
        write(&mock.join(rel), "# rev two\n");
        assert_ne!(
            before,
            now(&root, &mock),
            "{rel}: a committed lockfile moved"
        );
    }
}

#[test]
fn a_lockfile_that_is_tracked_and_then_removed_moves_the_digest() {
    let (_t, root, mock) = repo();
    let rel = "benches/sample/arms/a/Cargo.lock";
    write(&mock.join(rel), "# rev one\n");
    git(&root, &["add", "-A"]);
    let before = now(&root, &mock);
    fs::remove_file(mock.join(rel)).unwrap();
    assert_ne!(before, now(&root, &mock));
}
