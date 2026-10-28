//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one writer of an executable file.
//!
//! Linux refuses to exec a file that any process holds open for writing
//! (`ETXTBSY`, "Text file busy"). A process with several threads forks on one of
//! them while another has its script open for writing, and the child inherits
//! that descriptor until its own `exec`. The writer closes its end at once and
//! the script is run a moment later, by the process itself or by a hook that
//! `bash` or `git` reaches, and whoever still holds the inherited copy makes the
//! run fail. A retry at one call site does not cover a hook run by git.
//!
//! [`write_executable`] keeps the write out of the calling process: the body goes
//! to a scratch file nobody executes, and `cp` makes the executable in its own
//! process, which closes it before it exits. The mode is set by path afterwards,
//! which opens nothing. So no descriptor on the executable exists in a process
//! that forks, in the engine and in a test alike.
//!
//! What enforces that this is the only place a mode is set is
//! `tests/an_executable_is_written_by_the_one_writer.rs`, over every source
//! file of the repository.

use std::io;
use std::path::Path;

/// Write `body` at `path` as an executable file that is safe to exec at once.
///
/// A file already at `path` is replaced by a new one rather than written into,
/// so a run that still holds the old one open cannot make the rewrite fail.
/// Where the platform has no execute bit this is a plain write.
pub fn write_executable(path: &Path, body: impl AsRef<[u8]>) -> io::Result<()> {
    write_executable_at(path, body.as_ref())
}

#[cfg(not(unix))]
fn write_executable_at(path: &Path, body: &[u8]) -> io::Result<()> {
    std::fs::write(path, body)
}

#[cfg(unix)]
fn write_executable_at(path: &Path, body: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    if path.is_dir() {
        return Err(io::Error::other(format!(
            "{} is a directory",
            path.display()
        )));
    }
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!("mock_exec_src_{}_{n}", std::process::id()));
    std::fs::write(&scratch, body)?;
    // A copy onto a name that exists would truncate the old inode in place.
    let _ = std::fs::remove_file(path);
    let copied = Command::new("cp").arg(&scratch).arg(path).status();
    let _ = std::fs::remove_file(&scratch);
    if !copied?.success() {
        return Err(io::Error::other(format!("cp to {} failed", path.display())));
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mock_exec_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A program found on `PATH`, as bytes, to be written elsewhere and run.
    fn program(name: &str) -> Vec<u8> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let found = std::env::split_paths(&path)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no `{name}` on PATH"));
        std::fs::read(found).unwrap()
    }

    /// Threads that fork all the time while one writes a script and runs it
    /// straight away: the shape in which `fs::write` then `exec` died with
    /// "Text file busy". The race is statistical, so the rounds are many; with
    /// the plain write in place of `write_executable` this loop fails on most
    /// runs on four cores.
    #[test]
    fn a_script_written_while_other_threads_fork_runs_at_once() {
        let stop = AtomicBool::new(false);
        // Stops the forking threads when the loop panics too, or the scope never ends.
        struct Stop<'a>(&'a AtomicBool);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let dir = scratch_dir("race");
        thread::scope(|s| {
            let _stop = Stop(&stop);
            for _ in 0 .. 4 {
                s.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = Command::new("true").status();
                    }
                });
            }
            for round in 0 .. 400 {
                let script = dir.join(format!("hook_{round}.sh"));
                write_executable(&script, "#!/bin/sh\nexit 7\n").unwrap();
                let code = Command::new(&script)
                    .status()
                    .unwrap_or_else(|e| panic!("round {round}: {e}"))
                    .code();
                assert_eq!(code, Some(7), "round {round}");
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The case Linux does refuse: a binary that is being executed cannot be
    /// opened for writing. A `#!` script can, since only the interpreter is
    /// mapped, so the rewrite is tried against a copied binary. With the
    /// `remove_file` taken out of `write_executable` the second write fails
    /// with "Text file busy"; with it the old inode is left to the running
    /// process and the path names a new one.
    #[test]
    fn rewriting_a_binary_that_is_running_succeeds() {
        let dir = scratch_dir("rewrite");
        let bin = dir.join("prog");
        write_executable(&bin, program("sleep")).unwrap();
        let mut running = Command::new(&bin).arg("5").spawn().unwrap();
        assert!(
            running.try_wait().unwrap().is_none(),
            "the copy runs, or this proves nothing"
        );
        let rewritten = write_executable(&bin, program("true"));
        let _ = running.kill();
        let _ = running.wait();
        rewritten.unwrap();
        assert!(
            Command::new(&bin).status().unwrap().success(),
            "the path names the new file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_at_the_path_is_an_error_and_not_a_file_inside_it() {
        let dir = scratch_dir("isdir");
        assert!(write_executable(&dir, "#!/bin/sh\n").is_err());
        assert!(
            std::fs::read_dir(&dir).unwrap().next().is_none(),
            "nothing was put inside"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_is_executable_and_its_parents_are_made() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch_dir("mode");
        let hook = dir.join("a/b/hook");
        write_executable(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        assert_eq!(
            std::fs::metadata(&hook).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
