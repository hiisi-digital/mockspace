//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------


//! Writing a script a test then runs, without the exec-after-write race.
//!
//! Linux refuses to exec a file that any process holds open for writing
//! (`ETXTBSY`, "Text file busy"). The tests run on many threads of one process,
//! and a thread that forks while another has its script open for writing hands
//! the child a copy of that descriptor, held until the child's own `exec`. The
//! writer closes its end at once and runs the script a moment later, and
//! whoever still holds the inherited copy makes the run fail. The script may be
//! run by the test itself or by a hook that `bash` or `git` reaches, so a retry
//! at one call site does not cover it.
//!
//! `write_executable` keeps the write out of this process: the body goes to a
//! scratch file nobody executes, and `cp` makes the executable in its own
//! process, which closes it before it exits. No descriptor on the executable
//! ever exists in a process that forks.
//!
//! One file, included by path from the lib tests (`src/test_exec.rs`) and from
//! the integration tests, the way `ambient.rs` is. It names nothing but `std`.

#![allow(dead_code)]

use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// Write `body` at `path` as a mode 0o755 file that is safe to exec at once.
pub fn write_executable(path: &Path, body: impl AsRef<[u8]>) {
    use std::os::unix::fs::PermissionsExt;
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let src = std::env::temp_dir().join(format!("mock_exec_src_{}_{n}", std::process::id()));
    std::fs::write(&src, body).unwrap();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    // A copy onto a name that exists would truncate the old inode in place.
    let _ = std::fs::remove_file(path);
    let status = Command::new("cp").arg(&src).arg(path).status().expect("cp runs");
    let _ = std::fs::remove_file(&src);
    assert!(status.success(), "cp to {} failed", path.display());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}
