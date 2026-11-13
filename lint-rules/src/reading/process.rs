//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a started process answered, kept as the bytes it wrote.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// The answer of one command: whether it started, how it ended, and what it wrote.
///
/// Held once by the reading for every lint that asks the same question, and lent out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    started: bool,
    code:    Option<i32>,
    stdout:  Vec<u8>,
    stderr:  Vec<u8>,
}

impl Answer {
    /// Whether the program could be started at all. `false` for a program that is not on `PATH`.
    #[must_use]
    pub fn started(&self) -> bool {
        self.started
    }

    /// Whether it started and exited with status zero.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.started && self.code == Some(0)
    }

    /// The exit code, `None` when it did not start or was ended by a signal.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    /// What it wrote to its standard output.
    #[must_use]
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// What it wrote to its standard error.
    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// Its standard output as text, `None` where it is not UTF-8.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.stdout).ok()
    }

    /// Its standard output as text when it exited with status zero, which is what most callers of
    /// `git` want.
    #[must_use]
    pub fn ok_text(&self) -> Option<&str> {
        if self.ok() { self.text() } else { None }
    }

    /// The bytes the digest of this answer is taken over: the status and both outputs.
    pub(crate) fn material(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.stdout.len() + self.stderr.len() + 16);
        out.extend_from_slice(if self.started { b"started\0" } else { b"absent\0" });
        out.extend_from_slice(format!("{:?}\0", self.code).as_bytes());
        out.extend_from_slice(&(self.stdout.len() as u64).to_le_bytes());
        out.extend_from_slice(&self.stdout);
        out.extend_from_slice(&self.stderr);
        out
    }
}

/// Run `program` in `cwd` with `args`, writing `stdin` to it when given, and wait for it.
pub(crate) fn spawn(program: &str, args: &[String], cwd: &Path, stdin: Option<&[u8]>) -> Answer {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return Answer::default();
    };
    if let Some(input) = stdin {
        if let Some(mut pipe) = child.stdin.take() {
            // A writer thread, so a program that fills its output before reading all of its input
            // does not deadlock against this one.
            let input = input.to_vec();
            let writer = std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
            let out = child.wait_with_output();
            let _ = writer.join();
            return finish(out);
        }
    }
    finish(child.wait_with_output())
}

fn finish(out: std::io::Result<std::process::Output>) -> Answer {
    match out {
        Ok(out) => Answer {
            started: true,
            code:    out.status.code(),
            stdout:  out.stdout,
            stderr:  out.stderr,
        },
        Err(_) => Answer::default(),
    }
}
