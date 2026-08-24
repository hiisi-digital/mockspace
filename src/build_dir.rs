//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the engine puts generated build output, and why it is invisible to
//! the desktop indexer.
//!
//! Everything the engine generates goes under a `target/` directory: the lint
//! and tool cdylib crate, the hooks, the bench scaffolding, the registry
//! schemas. All derived, all churning on every run, none of it source anybody
//! searches for.
//!
//! Spotlight does not know that. Left unmarked, a build tree is a few thousand
//! new files per run for `mdworker` to open, read and index, and it costs real
//! machine while contributing nothing. Measured on one probe: two throwaway
//! build trees under `/private/tmp` put four `mdworker_shared` processes and
//! `mds` on the CPU at once, and `mds_stores` was still at 180% cleaning up
//! after they were deleted.
//!
//! A `.metadata_never_index` file in a directory takes that directory and
//! everything under it out of the index. Writing it is the engine's job rather
//! than the consumer's: a project should not have to know the desktop indexer
//! exists to avoid paying for it.
//!
//! **Asking where a thing goes and making it go there are separate.** [`target_dir`]
//! is a pure path computation and creates nothing. [`ensure`] creates and marks,
//! and is called only where a write actually follows. The split is not tidiness:
//! `bootstrap::activate` computes the hooks directory in order to test
//! `.exists()` and refuse, and a precondition check that creates the thing it is
//! checking for is a bug that reports success.

use std::fs;
use std::path::{Path, PathBuf};

/// The marker macOS reads. Harmless everywhere else: an empty dotfile.
const MARKER: &str = ".metadata_never_index";

/// `<root>/target`. Pure: creates nothing, writes nothing.
pub fn target_dir(root: &Path) -> PathBuf {
    root.join("target")
}

/// Create `dir` and take it out of the desktop index. Returns it, so a write
/// site reads as one expression.
///
/// NOTE: best effort throughout. an unwritable directory is a real failure and
/// the caller that needs it reports it far better than a panic here would.
pub fn ensure(dir: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&dir);
    mark(&dir);
    dir
}

/// `<root>/target`, created and out of the index.
pub fn ensure_target_dir(root: &Path) -> PathBuf {
    ensure(target_dir(root))
}

/// `<root>/target/<parts...>`, created, with the **`target/` root** marked.
///
/// NOTE: the mark goes on `target/` rather than on the subdirectory, so it
/// covers every generator including ones added later that never think about
/// this. Marking only the subdir was the first version's defect and it is
/// narrower than it reads.
pub fn ensure_under_target(root: &Path, parts: &[&str]) -> PathBuf {
    let mut dir = ensure_target_dir(root);
    for p in parts {
        dir = dir.join(p);
    }
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Take one existing directory out of the index, if it is not already.
///
/// Separate from [`ensure`] because of the case that motivates it: cargo honours
/// `CARGO_TARGET_DIR`, so the tree it writes is frequently **not** under any
/// path this engine computed. The artifact's own location is the only thing
/// that knows where it went, and that is learned from cargo rather than guessed.
pub fn mark(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    let marker = dir.join(MARKER);
    if !marker.exists() {
        let _ = fs::write(&marker, b"");
    }
}

/// Given a built artifact at `<somewhere>/<profile>/<lib>`, mark `<somewhere>`.
///
/// That is cargo's own layout: the artifact sits one directory below the target
/// root, in the profile directory. Anything shallower would mark a directory the
/// engine does not own, so a path that does not have two ancestors is left alone.
pub fn mark_target_root_of(artifact: &Path) -> Option<PathBuf> {
    let root = artifact.parent()?.parent()?.to_path_buf();
    mark(&root);
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_where_a_thing_goes_creates_nothing() {
        // the control the first version of this file did not have, and the
        // defect it did not have it for: `bootstrap::activate` computes the
        // hooks dir to test `.exists()` and refuse. a query that creates the
        // directory makes that check pass on a tree that has never been built.
        let tmp = tempfile::tempdir().unwrap();
        let dir = target_dir(tmp.path());
        assert_eq!(dir, tmp.path().join("target"));
        assert!(!dir.exists());
        assert!(!dir.join(MARKER).exists());
    }

    #[test]
    fn ensuring_creates_and_marks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        assert!(dir.is_dir());
        assert!(dir.join(MARKER).is_file());
    }

    #[test]
    fn an_existing_marker_is_left_alone() {
        // a person may have put something in it, or another tool may have. we
        // are not the only writer of this filename on a machine.
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        fs::write(dir.join(MARKER), b"do not clobber").unwrap();
        ensure_target_dir(tmp.path());
        assert_eq!(
            fs::read_to_string(dir.join(MARKER)).unwrap(),
            "do not clobber"
        );
    }

    #[test]
    fn the_mark_goes_on_the_target_root_not_the_subdirectory() {
        // the control against marking only what was asked for: a generator
        // added later writes a sibling subdir and inherits nothing.
        let tmp = tempfile::tempdir().unwrap();
        let sub = ensure_under_target(tmp.path(), &["mockspace", "registry-schemas"]);
        assert!(sub.is_dir());
        assert!(target_dir(tmp.path()).join(MARKER).is_file());
    }

    #[test]
    fn ensuring_twice_leaves_the_tree_alone() {
        // control against a helper that churns the thing it exists to stop
        // churning.
        let tmp = tempfile::tempdir().unwrap();
        let dir = ensure_target_dir(tmp.path());
        fs::write(dir.join("artifact"), b"x").unwrap();
        ensure_target_dir(tmp.path());
        assert!(dir.join("artifact").is_file());
    }

    #[test]
    fn an_artifact_marks_the_target_root_cargo_actually_used() {
        // the case the marker missed entirely: under CARGO_TARGET_DIR the tree
        // cargo writes is nowhere this engine computed, so marking
        // `<mock>/target` marks an almost-empty directory and leaves the
        // multi-gigabyte one indexed.
        let tmp = tempfile::tempdir().unwrap();
        let rel = tmp.path().join("elsewhere").join("release");
        fs::create_dir_all(&rel).unwrap();
        let artifact = rel.join("libmockspace_lints_abc.dylib");
        fs::write(&artifact, b"").unwrap();

        let marked = mark_target_root_of(&artifact).unwrap();
        assert_eq!(marked, tmp.path().join("elsewhere"));
        assert!(marked.join(MARKER).is_file());
        // and not the profile directory, which is cargo's and not a root
        assert!(!rel.join(MARKER).exists());
    }

    #[test]
    fn a_path_with_no_room_above_it_is_left_alone() {
        // control: no ancestors means no directory we can claim to own, and
        // marking one anyway would put the file somewhere arbitrary.
        assert!(mark_target_root_of(Path::new("lib.dylib")).is_none());
        // and marking a directory that does not exist writes nothing
        let tmp = tempfile::tempdir().unwrap();
        let absent = tmp.path().join("nope");
        mark(&absent);
        assert!(!absent.exists());
    }
}
