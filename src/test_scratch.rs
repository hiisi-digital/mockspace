//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where tests make their scratch directories: one root per test process under the system
//! temporary directory, `ms-test-scratch/<pid>/`. A test that panics or a run that is
//! killed leaves its root behind, and the first test of a later run removes every root that
//! nothing has added to for six hours, so a machine running the suite all day keeps only the
//! scratch of the runs going now.
//!
//! The integration tests share this file through `#[path]`, so there is one copy of the rule.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

/// The directory under the system temporary directory that holds every test process's root.
/// It carries no `mock` in its name: the write guard finds its target by that word anywhere in
/// a path (ticket 9e3ceccdb7a2), and a scratch path holding it would change what the guard tests see.
pub const SCRATCH_DIR: &str = "ms-test-scratch";

/// How long a root nothing has added to stays before a later run removes it.
const STALE: Duration = Duration::from_secs(6 * 60 * 60);

/// This process's scratch root, made on first use; the first call also removes the stale roots
/// of earlier processes.
pub fn root() -> PathBuf {
    static REAP: Once = Once::new();
    let base = std::env::temp_dir().join(SCRATCH_DIR);
    REAP.call_once(|| reap(&base, STALE));
    let mine = base.join(std::process::id().to_string());
    std::fs::create_dir_all(&mine).expect("make the test scratch root");
    mine
}

/// Removes every directory under `base` except this process's own whose modification time is
/// older than `stale`.
pub fn reap(base: &Path, stale: Duration) {
    let mine = std::process::id().to_string();
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for e in entries.flatten() {
        if e.file_name().to_str() == Some(mine.as_str()) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age > stale);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}
