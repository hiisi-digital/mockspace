//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The scratch repository the generated-file merge tests share.
//!
//! Every scenario runs the real binary and real git in a scratch repository,
//! with the generated hooks active and `cargo mock` and `mock` shimmed to this
//! build, so what is asserted is what a consumer's clone does.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const BIN: &str = env!("CARGO_BIN_EXE_mockspace");

pub struct Repo {
    pub _dir:  tempfile::TempDir,
    pub root:  PathBuf,
    pub home:  PathBuf,
    pub shims: PathBuf,
}

pub fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

impl Repo {
    pub fn new() -> Self {
        Self::ignoring("")
    }

    /// The same repository with `gitignore` already in place when the first
    /// generation runs, so a generated file it names is never tracked.
    pub fn ignoring(gitignore: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        let shims = base.join("shims");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        // `cargo mock` and `mock`, both this build. A real `cargo` is never
        // reached: nothing in these hooks asks for one.
        write(
            &shims.join("cargo"),
            "#!/bin/sh\n[ \"$1\" = mock ] && { shift; exec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"; }\nexit 127\n",
        );
        write(
            &shims.join("mock"),
            "#!/bin/sh\nexec \"$MOCKSPACE_BIN\" --dir \"$(git rev-parse --show-toplevel)/mock\" \"$@\"\n",
        );
        for s in ["cargo", "mock"] {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(shims.join(s), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let repo = Repo {
            _dir: dir,
            root,
            home,
            shims,
        };
        repo.git(&["init", "-q", "-b", "dev"]);
        repo.git(&["config", "user.name", "t"]);
        repo.git(&["config", "user.email", "t@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "merge.ff", "false"]);
        write(
            &repo.root.join("mock/Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        );
        write(
            &repo.root.join("mock/mockspace.toml"),
            "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n",
        );
        write(
            &repo.root.join("mock/ALPHA.md.tmpl"),
            "one\ntwo\nthree\nfour\nfive\n",
        );
        write(&repo.root.join("notes.txt"), "notes\n");
        if !gitignore.is_empty() {
            write(&repo.root.join(".gitignore"), gitignore);
        }
        repo.generate();
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "--no-verify", "-m", "chore: base"]);
        repo
    }

    pub fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        let path = format!(
            "{}:{}",
            self.shims.display(),
            std::env::var("PATH").unwrap()
        );
        c.current_dir(&self.root)
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("MOCKSPACE_BIN", BIN)
            .env_remove("MOCK_ROOT");
        c
    }

    pub fn git_out(&self, args: &[&str]) -> Output {
        self.cmd("git").args(args).output().unwrap()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = self.git_out(args);
        assert!(
            out.status.success(),
            "git {args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Generation alone, as a hook runs it, from inside the mock directory.
    pub fn generate(&self) {
        let out = self
            .cmd(BIN)
            .current_dir(self.root.join("mock"))
            .arg("--generate-only")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "generate: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Git in another working tree of the same repository.
    pub fn git_in(&self, dir: &Path, args: &[&str]) -> Output {
        self.cmd("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap()
    }

    /// A linked working tree on `branch`, with no `target/` in it: what a
    /// fresh `git worktree add` gives, and what `cargo clean` leaves.
    pub fn worktree(&self, branch: &str) -> PathBuf {
        let dir = self.root.parent().unwrap().join("wt");
        self.git(&["worktree", "add", "-q", dir.to_str().unwrap(), branch]);
        assert!(
            !dir.join("mock/target").exists(),
            "a fresh worktree has no target/"
        );
        dir
    }

    /// Generation alone, run from the mock directory of the working tree `dir`.
    pub fn generate_in(&self, dir: &Path) {
        let out = self
            .cmd(BIN)
            .current_dir(dir.join("mock"))
            .arg("--generate-only")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "generate: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn doc(&self) -> String {
        fs::read_to_string(self.root.join("docs/ALPHA.md")).unwrap()
    }

    pub fn edit_template(&self, from: &str, to: &str) {
        let path = self.root.join("mock/ALPHA.md.tmpl");
        let text = fs::read_to_string(&path).unwrap().replace(from, to);
        fs::write(path, text).unwrap();
    }

    /// Change the template, regenerate, commit. Waits out the second so the
    /// generation header differs between branches, which is what makes the
    /// documents conflict with no help from the body.
    pub fn side(&self, branch: &str, from: &str, to: &str) {
        self.edit_template(from, to);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        self.generate();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: edit on {branch}")]);
    }

    /// Two branches off the base, each changing a different template line, so the
    /// template merges cleanly and the generated document does not.
    pub fn two_sides(&self) {
        self.git(&["switch", "-q", "-c", "a"]);
        self.side("a", "one", "one, edited on a");
        self.git(&["switch", "-q", "dev"]);
        self.side("dev", "five", "five, edited on dev");
    }

    pub fn unmerged(&self) -> Vec<String> {
        self.git(&["diff", "--name-only", "--diff-filter=U"])
            .lines()
            .map(String::from)
            .collect()
    }

    pub fn status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }

    pub fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(String::from)
            .collect()
    }
}

impl Repo {
    /// A second generated document, from its own template, on a branch of its
    /// own. Two of these on two branches are two adds, each beside the other.
    pub fn add_document(&self, branch: &str, name: &str) {
        write(
            &self.root.join(format!("mock/{name}.md.tmpl")),
            &format!("{name} on {branch}\n"),
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        self.generate();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: add {name}")]);
    }

    pub fn failure(out: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }
}

impl Repo {
    /// A repository mockspace has not run in yet.
    pub fn untouched() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("home");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        let repo = Repo {
            _dir: dir,
            root,
            home,
            shims: base.join("shims"),
        };
        repo.git(&["init", "-q", "-b", "dev"]);
        write(
            &repo.root.join("mock/Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        );
        write(
            &repo.root.join("mock/mockspace.toml"),
            "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n",
        );
        repo
    }

    pub fn config(&self, key: &str) -> Option<String> {
        let out = self.git_out(&["config", "--local", "--get", key]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}
