//! The `git` command, run the same way every time.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::StoreError;

pub(crate) struct Git {
    dir: PathBuf,
}

pub(crate) struct Out {
    pub code:   Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_owned()
    }
}

impl Git {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
        }
    }

    /// Run `git -C <dir> <args>`. The environment is cleaned of the variables a
    /// hook sets (`GIT_DIR` and the like), which would send the command to a
    /// different repository; locale is fixed so git's words do not vary.
    pub fn run(
        &self,
        args: &[&str],
        envs: &[(&str, &str)],
        stdin: Option<&[u8]>,
    ) -> Result<Out, StoreError> {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.dir).args(args);
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_COMMON_DIR",
        ] {
            cmd.env_remove(var);
        }
        cmd.env("LC_ALL", "C").env("GIT_TERMINAL_PROMPT", "0");
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let io = |source| {
            StoreError::Io {
                context: format!("running `git {}`", args.join(" ")),
                source,
            }
        };
        let mut child = cmd.spawn().map_err(io)?;
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(bytes).map_err(io)?;
        }
        let out = child.wait_with_output().map_err(io)?;
        Ok(Out {
            code:   out.status.code(),
            stdout: out.stdout,
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }

    /// Like `run`, and a non-zero exit is a `StoreError::Git`.
    pub fn must(
        &self,
        args: &[&str],
        envs: &[(&str, &str)],
        stdin: Option<&[u8]>,
    ) -> Result<Out, StoreError> {
        let out = self.run(args, envs, stdin)?;
        if out.ok() {
            Ok(out)
        } else {
            Err(StoreError::Git {
                command: args.join(" "),
                detail:  out.stderr,
            })
        }
    }
}
