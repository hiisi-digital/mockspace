//! Where the engine puts generated build output, and why it is invisible to
//! the desktop indexer.
//!
//! Everything the engine generates goes under a `target/` directory: the lint
//! and tool cdylib crate, the hooks, the bench scaffolding, the registry
//! schemas. All of it is derived, all of it churns on every run, and none of it
//! is source anybody searches for.
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

use std::fs;
use std::path::{Path, PathBuf};

/// The marker macOS reads. Harmless everywhere else: an empty dotfile.
const MARKER: &str = ".metadata_never_index";

/// `<root>/target`, created, and excluded from the index.
///
/// NOTE: the marker goes on `target/` rather than on each generated
/// subdirectory, so one write covers every generator, including ones added
/// later that never think about this.
pub fn target_dir(root: &Path) -> PathBuf {
    let dir = root.join("target");
    // best effort throughout. an unwritable target dir is a real failure, and
    // the caller that actually needs it reports it far better than a panic
    // here would.
    let _ = fs::create_dir_all(&dir);
    let marker = dir.join(MARKER);
    if !marker.exists() {
        let _ = fs::write(&marker, b"");
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_lands_and_the_directory_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = target_dir(tmp.path());
        assert_eq!(dir, tmp.path().join("target"));
        assert!(dir.is_dir());
        assert!(dir.join(MARKER).is_file());
    }

    #[test]
    fn an_existing_marker_is_left_alone() {
        // a person may have put something in it, or a tool may have. we are
        // not the only writer of this filename on a machine.
        let tmp = tempfile::tempdir().unwrap();
        let dir = target_dir(tmp.path());
        fs::write(dir.join(MARKER), b"do not clobber").unwrap();
        target_dir(tmp.path());
        assert_eq!(
            fs::read_to_string(dir.join(MARKER)).unwrap(),
            "do not clobber"
        );
    }

    #[test]
    fn it_is_idempotent_over_an_existing_tree() {
        // the control: a second call must not disturb what is already there,
        // else every run churns the tree it exists to stop churning.
        let tmp = tempfile::tempdir().unwrap();
        let dir = target_dir(tmp.path());
        fs::write(dir.join("artifact"), b"x").unwrap();
        target_dir(tmp.path());
        assert!(dir.join("artifact").is_file());
    }
}
