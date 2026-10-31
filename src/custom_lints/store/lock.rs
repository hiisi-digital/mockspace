//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The lock a run holds on a key directory for the write, the build and the load.

use std::fs::File;
use std::path::Path;

#[cfg(test)]
use super::knobs;
use super::LOCK;

/// What goes wrong when the lock cannot be had, said once for both places it
/// can fail.
fn lock_hint(gen_dir: &Path) -> String {
    format!(
        "the shared lint crate directory {} needs a writable filesystem that supports file \
         locks; point XDG_CACHE_HOME or CARGO_TARGET_DIR at one that does",
        gen_dir.display()
    )
}

/// Create `gen_dir` and hold its lock for this run. Released when the file is
/// dropped.
///
/// The lock is only worth having if the file locked is the file [`reap`] would
/// delete, so once held it is checked still to be the one at the path. A reaper
/// that removed the directory between our open and our lock leaves us holding
/// an unlinked file, and a second process locking the recreated one would then
/// run beside us; here that loops and starts again.
pub(crate) fn lock_dir(gen_dir: &Path) -> Result<File, String> {
    let path = gen_dir.join(LOCK);
    loop {
        std::fs::create_dir_all(gen_dir).map_err(|e| {
            format!(
                "could not create {}: {e}; {}",
                gen_dir.display(),
                lock_hint(gen_dir)
            )
        })?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| {
                format!(
                    "could not open {}: {e}; {}",
                    path.display(),
                    lock_hint(gen_dir)
                )
            })?;
        #[cfg(test)]
        if knobs::skips_the_lock(gen_dir) {
            return Ok(file);
        }
        file.lock().map_err(|e| {
            format!(
                "could not lock {}: {e}; {}",
                path.display(),
                lock_hint(gen_dir)
            )
        })?;
        if is_the_file_at(&file, &path) {
            return Ok(file);
        }
    }
}

#[cfg(unix)]
pub(super) fn is_the_file_at(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(held), Ok(there)) => held.dev() == there.dev() && held.ino() == there.ino(),
        _ => false,
    }
}

// Without inodes there is nothing to compare, and the reaper's removal of a
// directory that is locked is refused by the platform on these anyway.
#[cfg(not(unix))]
pub(super) fn is_the_file_at(_file: &File, path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
