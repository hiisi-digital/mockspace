//! A bare remote and clones of it, driven through the real binary and real git.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub const BIN: &str = env!("CARGO_BIN_EXE_mockspace");

static CLOCK: AtomicU64 = AtomicU64::new(0);

/// A deterministic, strictly increasing commit time.
fn tick() -> String {
    let n = CLOCK.fetch_add(60, Ordering::SeqCst);
    format!("{} +0000", 1_790_000_000 + n)
}

pub struct Out {
    pub ok:     bool,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub struct World {
    pub tmp:    tempfile::TempDir,
    pub remote: PathBuf,
    pub home:   PathBuf,
}

impl World {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let remote = base.join("remote.git");
        let home = base.join("home");
        fs::create_dir_all(&home).unwrap();
        let w = World { tmp, remote: remote.clone(), home };
        let shims = base.join("shims");
        fs::create_dir_all(&shims).unwrap();
        mockspace_manifest::write_executable(
            &shims.join("cargo"),
            "#!/bin/sh\n[ \"$1\" = mock ] && { shift; exec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"; }\n[ \"$1\" = metadata ] && exec \"$REAL_CARGO\" \"$@\"\nexit 127\n",
        )
        .unwrap();
        mockspace_manifest::write_executable(
            &shims.join("mock"),
            "#!/bin/sh\nexec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"\n",
        )
        .unwrap();
        fs::create_dir_all(&remote).unwrap();
        assert!(w.sh(&remote, "git", &["init", "-q", "--bare", "-b", "dev"]).ok);
        w
    }

    pub fn base(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }

    pub fn command(&self, program: &str, dir: &Path) -> Command {
        let mut c = Command::new(program);
        c.current_dir(dir)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("MOCK_ROOT")
            .env("MOCKSPACE_BIN", BIN)
            .env("REAL_CARGO", std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .env("PATH", format!("{}:{}", self.base().join("shims").display(), std::env::var("PATH").unwrap()));
        let t = tick();
        for k in ["GIT_AUTHOR_DATE", "GIT_COMMITTER_DATE"] {
            c.env(k, &t);
        }
        for (k, v) in [("NAME", "tester"), ("EMAIL", "tester@example.com")] {
            c.env(format!("GIT_AUTHOR_{k}"), v);
            c.env(format!("GIT_COMMITTER_{k}"), v);
        }
        c
    }

    pub fn sh(&self, dir: &Path, program: &str, args: &[&str]) -> Out {
        let o = self.command(program, dir).args(args).output().unwrap();
        Out {
            ok:     o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    /// git against the bare remote.
    pub fn remote_git(&self, args: &[&str]) -> String {
        let o = self.sh(&self.remote, "git", args);
        assert!(o.ok, "remote git {args:?}: {}", o.all());
        o.stdout
    }

    pub fn remote_tip(&self, reference: &str) -> Option<String> {
        let o = self.sh(&self.remote, "git", &["rev-parse", "--verify", "-q", reference]);
        o.ok.then(|| o.stdout.trim().to_string())
    }

    /// A clone of the remote on `dev`, configured. The remote must have a `dev`.
    pub fn clone_of(&self, name: &str) -> Clone<'_> {
        let root = self.base().join(name);
        let o = self.sh(&self.base(), "git", &["clone", "-q", self.remote.to_str().unwrap(), name]);
        assert!(o.ok, "clone: {}", o.all());
        let c = Clone { w: self, root };
        c.configure();
        c
    }

    /// A first clone of an empty remote, for building the fixture in.
    pub fn first_clone(&self, name: &str) -> Clone<'_> {
        let root = self.base().join(name);
        let o = self.sh(&self.base(), "git", &["clone", "-q", self.remote.to_str().unwrap(), name]);
        assert!(o.ok, "clone: {}", o.all());
        let c = Clone { w: self, root };
        c.configure();
        c.git(&["checkout", "-q", "-B", "dev"]);
        c
    }
}

pub struct Clone<'w> {
    pub w:    &'w World,
    pub root: PathBuf,
}

impl Clone<'_> {
    fn configure(&self) {
        for kv in [
            ["user.name", "tester"],
            ["user.email", "tester@example.com"],
            ["commit.gpgsign", "false"],
            ["core.autocrlf", "false"],
            ["core.quotepath", "false"],
            ["core.symlinks", "true"],
            ["core.filemode", "true"],
            ["push.default", "simple"],
        ] {
            self.git(&["config", kv[0], kv[1]]);
        }
    }

    pub fn mock(&self) -> PathBuf {
        self.root.join("mock")
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn git_out(&self, args: &[&str]) -> Out {
        self.w.sh(&self.root, "git", args)
    }

    pub fn git(&self, args: &[&str]) -> String {
        let o = self.git_out(args);
        assert!(o.ok, "git {args:?} in {}: {}", self.root.display(), o.all());
        o.stdout
    }

    /// Stage everything (ignored files stay out) and commit, bypassing the generated hooks, which
    /// are not what these scenarios are about.
    pub fn commit_all(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--no-verify", "--allow-empty", "-m", msg]);
    }

    /// Commit what is staged, bypassing the generated hooks.
    pub fn commit_staged(&self, msg: &str) {
        self.git(&["commit", "-q", "--no-verify", "-m", msg]);
    }

    pub fn push(&self) {
        self.git(&["push", "-q", "--no-verify", "origin", "HEAD"]);
    }

    /// The real binary, pointed at this clone's mock directory.
    pub fn run(&self, args: &[&str]) -> Out {
        self.run_in(&self.root, args)
    }

    pub fn run_in(&self, dir: &Path, args: &[&str]) -> Out {
        let mut c = self.w.command(BIN, dir);
        c.arg("--dir").arg(self.mock()).args(args);
        let o = c.output().unwrap();
        Out {
            ok:     o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    pub fn ok(&self, args: &[&str]) -> Out {
        let o = self.run(args);
        assert!(o.ok, "mockspace {args:?} in {}: {}", self.root.display(), o.all());
        o
    }

    pub fn fails(&self, args: &[&str]) -> Out {
        let o = self.run(args);
        assert!(!o.ok, "mockspace {args:?} should have refused: {}", o.all());
        o
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    pub fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.path(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"))
    }

    pub fn exists(&self, rel: &str) -> bool {
        fs::symlink_metadata(self.path(rel)).is_ok()
    }

    pub fn remove(&self, rel: &str) {
        let p = self.path(rel);
        if fs::symlink_metadata(&p).map(|m| m.is_dir()).unwrap_or(false) {
            fs::remove_dir_all(p).unwrap();
        } else {
            fs::remove_file(p).unwrap();
        }
    }

    /// Paths the branch tracks at HEAD, NUL-split so no name is quoted.
    pub fn tracked(&self) -> Vec<String> {
        self.git(&["ls-files", "-z"]).split('\0').filter(|s| !s.is_empty()).map(str::to_owned).collect()
    }

    pub fn untracked(&self) -> Vec<String> {
        self.git(&["ls-files", "-z", "--others", "--exclude-standard"])
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// A linked worktree of this clone, as a clone of its own for running commands in.
    pub fn worktree_add(&self, name: &str, branch: &str) -> PathBuf {
        let p = self.w.base().join(name);
        self.git(&["worktree", "add", "-q", "-b", branch, p.to_str().unwrap(), "HEAD"]);
        p
    }
}

impl<'w> Clone<'w> {
    /// A handle on another working tree of the same repository, for running commands in it.
    pub fn at(&self, root: PathBuf) -> Clone<'w> {
        Clone { w: self.w, root }
    }
}
