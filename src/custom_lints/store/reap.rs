//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Removing the key directories nobody can come back to.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::key::is_key_name;
use super::{LOCK, OWNER, USED};

/// Whether a key directory has been abandoned: the worktree that wrote it last
/// no longer exists, or nothing has used it for longer than `stale_after`.
///
/// A directory with no recorded owner is judged on age alone, which is what
/// keeps one a peer has created and not yet written from being taken.
fn abandoned(dir: &Path, now: SystemTime, stale_after: Duration) -> bool {
    if let Ok(owner) = std::fs::read_to_string(dir.join(OWNER))
        && !Path::new(owner.trim()).exists()
    {
        return true;
    }
    let last_used = std::fs::metadata(dir.join(USED))
        .or_else(|_| std::fs::metadata(dir))
        .and_then(|m| m.modified());
    match last_used {
        Ok(at) => now.duration_since(at).is_ok_and(|age| age > stale_after),
        // Not knowing is not a reason to delete.
        Err(_) => false,
    }
}

/// Remove the abandoned key directories under `base`, except `keep`, and return
/// the ones removed.
///
/// A key in use is never removed: each candidate's lock is taken without
/// waiting, and one that is held is skipped. The abandonment is judged only once
/// the lock is held, so a run that started between the listing and the lock
/// has already stamped the directory by then. Removal is best effort; a
/// directory that will not go is tried again on the next run.
pub(crate) fn reap(
    base: &Path,
    keep: &Path,
    now: SystemTime,
    stale_after: Duration,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir == keep || !is_key_name(&entry.file_name().to_string_lossy()) || !dir.is_dir() {
            continue;
        }
        // No lock file means no run has ever locked it, so there is no holder
        // to wait for; the age test then has to stand on the directory alone.
        let held = match File::open(dir.join(LOCK)) {
            Ok(file) => {
                match file.try_lock() {
                    Ok(()) => Some(file),
                    // Held by a run, or not lockable at all: leave it.
                    Err(_) => continue,
                }
            },
            Err(_) => None,
        };
        if abandoned(&dir, now, stale_after) && std::fs::remove_dir_all(&dir).is_ok() {
            removed.push(dir);
        }
        drop(held);
    }
    removed
}

#[cfg(test)]
#[path = "reap_tests.rs"]
mod tests;
