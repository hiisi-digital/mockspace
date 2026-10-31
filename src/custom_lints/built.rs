//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Built lint libraries kept by identity.
//!
//! A library is kept under the identity `identity` made for it, so a second tree of the same
//! content loads the first one's build and neither rebuilds nor relinks. What is kept is only ever
//! reachable by an identity that names exactly its inputs, so a stale entry is unreachable and
//! never wrong, and [`reap`] removes the ones nothing has used for a while.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DIR: &str = "built";
const EXTENSIONS: [&str; 3] = ["so", "dylib", "dll"];

fn kept(base: &Path, identity: &str) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|e| base.join(DIR).join(format!("{identity}.{e}")))
        .find(|p| p.is_file())
}

/// The library for `identity`: the kept one, stamped as used, or what `build` makes, kept first.
/// A library that cannot be kept is still returned, so a read-only store costs sharing only.
pub(crate) fn dylib_for(
    base: &Path,
    identity: &str,
    build: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    if let Some(path) = kept(base, identity) {
        if let Ok(file) = std::fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        return Ok(path);
    }
    let built = build()?;
    let ext = built.extension().and_then(|e| e.to_str()).unwrap_or("so");
    let dir = base.join(DIR);
    let target = dir.join(format!("{identity}.{ext}"));
    // Written beside and renamed in, so a peer never maps a half-written library.
    let tmp = dir.join(format!(".{identity}.{}.tmp", std::process::id()));
    let stored = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::copy(&built, &tmp))
        .and_then(|_| std::fs::rename(&tmp, &target));
    if stored.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(built)
}

/// Remove the kept libraries unused for longer than `stale_after`, and return how many.
pub(crate) fn reap(base: &Path, now: SystemTime, stale_after: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(base.join(DIR)) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > stale_after));
        if old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
#[path = "built_tests.rs"]
mod tests;
