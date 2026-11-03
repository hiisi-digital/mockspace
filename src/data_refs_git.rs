//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The calls a [`DataRef`] makes to git and to the repository's own files: its index file, its
//! exclude line, the paths it resolves and the one place a git process is spawned with the
//! ambient `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` taken out of its environment.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::DataRef;

impl DataRef {
    /// The index file the instance is mapped by, in the git directory so no worktree shares it.
    pub(super) fn index_file(&self) -> Result<PathBuf, String> {
        let dir = self.run(&["rev-parse", "--absolute-git-dir"])?;
        Ok(PathBuf::from(dir.trim()).join(format!("mock-{}.index", self.name)))
    }

    /// Adds `dir` to the repository's own exclude file when it sits inside the repository.
    pub(super) fn exclude(&self, dir: &Path) -> Result<(), String> {
        let Ok(rel) = dir.strip_prefix(self.absolute(Path::new(""))) else {
            return Ok(());
        };
        let line = format!("/{}/", rel.display());
        let git_dir = PathBuf::from(self.run(&["rev-parse", "--absolute-git-dir"])?.trim());
        let file = git_dir.join("info").join("exclude");
        let held = std::fs::read_to_string(&file).unwrap_or_default();
        if held.lines().any(|l| l == line) {
            return Ok(());
        }
        std::fs::create_dir_all(git_dir.join("info")).map_err(|e| e.to_string())?;
        let sep = if held.is_empty() || held.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&file, format!("{held}{sep}{line}\n")).map_err(|e| e.to_string())
    }

    pub(super) fn absolute(&self, dir: &Path) -> PathBuf {
        if dir.is_absolute() { dir.to_path_buf() } else { self.repo.join(dir) }
    }

    /// Runs git at the repository and gives its output, failing with git's words.
    pub(super) fn run(&self, args: &[&str]) -> Result<String, String> {
        self.git(&self.repo, args, None)?
            .ok_or_else(|| format!("git {} found nothing", args.join(" ")))
    }

    /// Runs git with `dir` as the work tree and this ref's own index file. The branch's index is
    /// never named, and an ambient one is removed from the environment.
    pub(super) fn indexed(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let index = self.index_file()?;
        let git_dir = self.run(&["rev-parse", "--absolute-git-dir"])?;
        let git_dir_arg = format!("--git-dir={}", git_dir.trim());
        let work_arg = format!("--work-tree={}", dir.display());
        let mut all = vec![git_dir_arg.as_str(), work_arg.as_str()];
        all.extend_from_slice(args);
        self.git_raw(dir, &all, Some(&index))?
            .ok_or_else(|| format!("git {} found nothing", args.join(" ")))
    }

    pub(super) fn git(
        &self,
        cwd: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let mut all = vec!["-C", cwd.to_str().ok_or("a path that is not text")?];
        all.extend_from_slice(args);
        self.git_raw(cwd, &all, index)
    }

    /// Nothing for a command that exits 1 with no output (`rev-parse -q` of a missing ref),
    /// the output of one that succeeds, the error of the rest.
    pub(super) fn git_raw(
        &self,
        cwd: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("git could not be run: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(Some(stdout));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.code() == Some(1) && stdout.is_empty() && stderr.trim().is_empty() {
            return Ok(None);
        }
        Err(format!("git {}: {}", args.join(" "), stderr.trim()))
    }
}
