//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a commit or push run changes, for the lints that declare the paths they read.
//!
//! A lint declaring [`Reach::Paths`](mockspace_lint_rules::Reach::Paths) is run only by a run
//! changing a file its globs match (the lint lists its own source and tooling in those globs). This
//! module reduces the run's [`Changed`] to that list, and decides when the list is not enough.
//! **Every lint runs** when:
//!
//! - the run is a plain one, or git could not list what changes (`Changed::Unknown`);
//! - a changed file is part of the lint set: `mockspace.toml` anywhere, the mock directory's
//!   `lints/` and `tools/`, and the toolchain pin and cargo config of the mock directory and of the
//!   repository root, and the mock directory's manifest and lockfile;
//! - the lint set moved without a file changing, or this tree was never judged under it. A pack
//!   pinned to a branch is re-resolved hourly, so a new or stricter lint can arrive with no tracked
//!   file touched. After a clean full run the key of the engine and packs
//!   (`DiskVerdicts::lint_set_key`) is kept with the `HEAD` commit it covered, for **this worktree**
//!   (its canonical root) and **this mode** (a commit run sees a push-gate finding only as a warning,
//!   so its clean run says nothing to the push gate). A later run narrows only when the key is the
//!   same and the kept commit is `HEAD` or an ancestor of it; another key, another worktree, a
//!   branch switch to a tree that does not descend from it, or no key kept, runs everything. A run
//!   that found something blocking keeps nothing, so the retry is full again.
//!
//! The notion of what changed is `registry_scope::Changed`, the one the row reference gate uses.

use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_lint_rules::{LintMode, Touched};

use super::verdicts::DiskVerdicts;
use crate::entry::registry_scope::Changed;

/// Where the key of the last clean full run is kept, and what this run would keep.
pub(crate) struct Marker {
    pub(crate) path: PathBuf,
    pub(crate) key:  String,
    /// The `HEAD` commit of this run, or `None` where there is none.
    pub(crate) head: Option<String>,
    pub(crate) root: PathBuf,
}

/// What one run changes, and what it keeps once it has run.
pub(crate) struct Plan {
    /// The repository-relative files changed, or `None` where every lint runs.
    files:  Option<Vec<String>>,
    marker: Option<Marker>,
}

fn git_line(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl Marker {
    /// The marker of a run in `mode` over the worktree at `repo_root`.
    fn of(mode: LintMode, repo_root: &Path, v: &DiskVerdicts) -> Self {
        let root = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
        let tag = crate::pack_pin::fnv1a(root.to_string_lossy().as_bytes());
        let mode = format!("{mode:?}").to_lowercase();
        Self {
            path: v.dir().join("last-full-run").join(format!("{tag:016x}-{mode}")),
            key:  v.lint_set_key(),
            head: git_line(&root, &["rev-parse", "--verify", "HEAD"]),
            root,
        }
    }

    /// Whether the key kept covers this run: the same key, over `HEAD` or something it descends from.
    fn covers(&self, kept_ancestor: &dyn Fn(&str) -> bool) -> bool {
        let Some(head) = &self.head else { return false };
        let Ok(text) = std::fs::read_to_string(&self.path) else { return false };
        let mut lines = text.lines();
        let (Some(key), Some(kept)) = (lines.next(), lines.next()) else { return false };
        key == self.key && (kept == head || kept_ancestor(kept))
    }
}

impl Plan {
    /// The plan of a run in `mode` over `repo_root`, with `verdicts` the store the run opened.
    pub(crate) fn of_run(
        mode: LintMode,
        repo_root: &Path,
        mock_dir: &Path,
        verdicts: Option<&DiskVerdicts>,
    ) -> Self {
        let files = match mode {
            LintMode::Build => None,
            _ => reduce(&Changed::of_run(mode, repo_root), repo_root, mock_dir),
        };
        let marker = verdicts.map(|v| Marker::of(mode, repo_root, v));
        let root = marker.as_ref().map(|m| m.root.clone());
        let ancestor = |kept: &str| {
            root.as_ref().is_some_and(|root| {
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(["merge-base", "--is-ancestor", kept, "HEAD"])
                    .status()
                    .is_ok_and(|s| s.success())
            })
        };
        Self::decide(files, marker, &ancestor)
    }

    /// The plan for `files` and a marker, a key that does not cover the run making it full.
    pub(crate) fn decide(
        files: Option<Vec<String>>,
        marker: Option<Marker>,
        kept_ancestor: &dyn Fn(&str) -> bool,
    ) -> Self {
        let covered = marker.as_ref().is_some_and(|m| m.covers(kept_ancestor));
        Self { files: if covered { files } else { None }, marker }
    }

    /// The scope the repo lints are handed.
    pub(crate) fn touched(&self) -> Touched<'_> {
        self.files.as_deref().map_or(Touched::Everything, Touched::Files)
    }

    /// Keep the key once a full run found nothing blocking. A doc-only run skips the source lints,
    /// so it was not full.
    pub(crate) fn settle(&self, blocking: usize, doc_only: bool) {
        if self.files.is_some() || blocking > 0 || doc_only {
            return;
        }
        let Some(m) = &self.marker else { return };
        let Some(head) = &m.head else { return };
        if let Some(dir) = m.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // The root is on the last line so the sweep can tell a worktree that is gone.
        let _ = std::fs::write(&m.path, format!("{}\n{head}\n{}\n", m.key, m.root.display()));
    }
}

/// The repository-relative files a run changes, or `None` where every lint runs.
pub(crate) fn reduce(changed: &Changed, repo_root: &Path, mock_dir: &Path) -> Option<Vec<String>> {
    let Changed::Files(paths) = changed else {
        return None;
    };
    let mock = mock_dir.strip_prefix(repo_root).unwrap_or(Path::new(""));
    let set_dirs = [mock.join("lints"), mock.join("tools")];
    let set_files = [
        mock.join("Cargo.toml"),
        mock.join("Cargo.lock"),
        mock.join("rust-toolchain.toml"),
        mock.join(".cargo/config.toml"),
        PathBuf::from("rust-toolchain.toml"),
        PathBuf::from(".cargo/config.toml"),
    ];
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let in_set = rel.file_name().is_some_and(|n| n == "mockspace.toml")
            || set_files.iter().any(|f| rel == f)
            || set_dirs.iter().any(|d| rel.starts_with(d));
        if in_set {
            return None;
        }
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    Some(files)
}

#[cfg(test)]
#[path = "touched_tests.rs"]
mod tests;
