//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------
//! Where tests make their scratch directories: one root per test process under the system
//! temporary directory, `ms-test-scratch/<pid>/`. A test that panics or a run that is killed
//! leaves its root behind. The first test of a later run removes the roots whose process has
//! ended and that nothing has added to for six hours, and never a root whose process still
//! runs, however old. A process whose id an ended run had takes that root over emptied.
//! Off unix nothing tells whether a process exists, so there no root is ever removed and the
//! roots of every run stay until somebody clears them.
//!
//! Here, in the one package every test target reaches and that ships whole, behind the
//! internal `_internal-test-scratch` feature, so there is one copy of the rule and a published
//! test reads it from inside the package. Unstable and no part of the public API.

use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::{Duration, SystemTime};

/// The directory under the system temporary directory that holds every test process's root.
/// It carries no `mock` in its name: the write guard finds its target by that word anywhere in
/// a path (ticket 9e3ceccdb7a2), and a scratch path holding it would change what the guard tests see.
pub const SCRATCH_DIR: &str = "ms-test-scratch";

/// How long a root nothing has added to stays before a later run removes it.
const STALE: Duration = Duration::from_secs(6 * 60 * 60);

/// This process's scratch root. The first call removes the stale roots of ended processes and
/// empties this process's own, which an ended run with the same id may have left.
pub fn root() -> PathBuf {
    static FIRST: Once = Once::new();
    let base = std::env::temp_dir().join(SCRATCH_DIR);
    let mine = base.join(std::process::id().to_string());
    FIRST.call_once(|| {
        reap(&base, STALE);
        claim(&mine);
    });
    mine
}

/// A fresh scratch directory under this process's root, removed when the handle drops.
pub fn tempdir() -> std::io::Result<tempfile::TempDir> {
    tempfile::tempdir_in(root())
}

/// Removes every root under `base` whose process has ended and whose modification time is
/// older than `stale`. This process's own root, and the root of any process still running,
/// stay whatever their age; a name that is no process id is left alone.
pub fn reap(base: &Path, stale: Duration) {
    let mine = std::process::id();
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == mine || alive(pid) {
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

/// Makes `dir` this process's own, emptied of whatever an ended run with the same id left in it.
pub fn claim(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("make the test scratch root");
}

/// Whether a process with id `pid` exists.
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Whether a process with id `pid` exists, asked by signal 0, which checks and sends nothing.
#[cfg(all(unix, not(target_os = "linux")))]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Where nothing says whether a process exists, every root counts as live and stays, so off
/// unix nothing is ever reaped.
#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests;
