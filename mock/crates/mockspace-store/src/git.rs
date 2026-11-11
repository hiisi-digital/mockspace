//! The `git` command, run the same way every time.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::StoreError;

#[derive(Clone)]
/// `git` run in one directory, the same way every time.
pub struct Git {
    dir:     PathBuf,
    program: std::ffi::OsString,
}

/// What a `git` call printed and how it ended.
pub struct Out {
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
            program: "git".into(),
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
        let mut cmd = Command::new(&self.program);
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
        cmd.env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_LITERAL_PATHSPECS", "1");
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

impl Git {
    /// The top of the work tree this runs in, or `None` where git says there is none: a bare
    /// repository, or a directory in no repository. Any other failure is an error, and never read as
    /// the answer that there is no work tree.
    pub fn work_tree_top(&self) -> Result<Option<String>, StoreError> {
        let out = self.run(&["rev-parse", "--show-toplevel"], &[], None)?;
        if out.ok() {
            return Ok(Some(out.text()));
        }
        let none = out.code == Some(128)
            && (out.stderr.contains("must be run in a work tree") || out.stderr.contains("not a git repository"));
        if none {
            return Ok(None);
        }
        Err(StoreError::Git {
            command: "rev-parse --show-toplevel".to_owned(),
            detail:  out.stderr,
        })
    }

    /// The value of a config key as `git config <args> --get <key>` gives it, or `None` where it is
    /// not set. Any other failure is an error, and never read as the key being unset.
    pub fn config_get(&self, args: &[&str], key: &str) -> Result<Option<String>, StoreError> {
        let mut all = vec!["config"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--get", key]);
        let out = self.run(&all, &[], None)?;
        match out.code {
            Some(0) => Ok(Some(out.text())),
            Some(1) if out.stderr.is_empty() => Ok(None),
            _ => {
                Err(StoreError::Git {
                    command: all.join(" "),
                    detail:  out.stderr,
                })
            },
        }
    }

    /// Run this program in place of `git`: a wrapper, or a build of git kept elsewhere.
    pub fn with_program(mut self, program: impl Into<std::ffi::OsString>) -> Self {
        self.program = program.into();
        self
    }

    /// The same program run in `dir` instead.
    pub(crate) fn in_dir(&self, dir: PathBuf) -> Self {
        Self {
            dir,
            program: self.program.clone(),
        }
    }
}
