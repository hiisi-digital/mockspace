//! Writing a script the tests then run, without the exec-after-write race.
//!
//! The one `write_executable` lives in `tests/common/exec.rs`, which the
//! integration tests include by path as well, so there is a single copy. Its
//! header says what the race is and how the write avoids it.

use std::{path::Path, process::Command};

#[path = "../tests/common/exec.rs"]
mod exec;

/// Write `body` at `path` as a mode 0o755 file that is safe to exec at once.
pub(crate) fn write_executable(path: &Path, body: impl AsRef<[u8]>) {
    exec::write_executable(path, body);
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    use super::*;

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
        let dir = std::env::temp_dir().join(format!("mock_exec_race_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
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
                write_executable(&script, "#!/bin/sh\nexit 7\n");
                let code = Command::new(&script)
                    .status()
                    .unwrap_or_else(|e| panic!("round {round}: {e}"))
                    .code();
                assert_eq!(code, Some(7), "round {round}");
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A script written over an existing name is a new file, so an old run's
    /// hold on the previous one cannot make the rewrite fail.
    #[test]
    fn rewriting_a_script_that_is_running_succeeds() {
        let dir = std::env::temp_dir().join(format!("mock_exec_rewrite_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("hook.sh");
        write_executable(&script, "#!/bin/sh\nsleep 2\n");
        let mut running = Command::new(&script).spawn().unwrap();
        write_executable(&script, "#!/bin/sh\nexit 3\n");
        assert_eq!(Command::new(&script).status().unwrap().code(), Some(3));
        let _ = running.kill();
        let _ = running.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
