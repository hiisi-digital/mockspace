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
//!   `lints/` and `tools/`, and the manifest, lockfile, toolchain pin and cargo config of the mock
//!   directory and of the repository root;
//! - the lint set moved without a file changing. A pack pinned to a branch is re-resolved hourly, so
//!   a new or stricter lint can arrive with no tracked file touched. The key of the engine and
//!   packs (`DiskVerdicts::lint_set_key`) is kept per repository after every full run that found
//!   nothing blocking, and a run finding another key is full. A run that found something leaves
//!   the key as it was, so the retry is full again.
//!
//! The notion of what changed is `registry_scope::Changed`, the one the row reference gate uses.

use std::path::{Path, PathBuf};

use mockspace_lint_rules::{LintMode, Touched};

use super::verdicts::DiskVerdicts;
use crate::entry::registry_scope::Changed;

/// What one run changes, and what it keeps once it has run.
pub(crate) struct Plan {
    /// The repository-relative files changed, or `None` where every lint runs.
    files:  Option<Vec<String>>,
    /// Where the lint set key of the last full run is kept, and the key of this run.
    marker: Option<(PathBuf, String)>,
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
        let marker = verdicts.map(|v| {
            let tag = crate::pack_pin::fnv1a(crate::custom_lints::store::repo_identity(repo_root).as_bytes());
            (v.dir().join("last-full-run").join(format!("{tag:016x}")), v.lint_set_key())
        });
        Self::decide(files, marker)
    }

    /// The plan for `files` and a marker, a moved or missing key making it full.
    pub(crate) fn decide(files: Option<Vec<String>>, marker: Option<(PathBuf, String)>) -> Self {
        let moved = marker
            .as_ref()
            .is_none_or(|(path, key)| std::fs::read_to_string(path).ok().as_deref() != Some(key));
        Self { files: if moved { None } else { files }, marker }
    }

    /// The scope the repo lints are handed.
    pub(crate) fn touched(&self) -> Touched<'_> {
        self.files.as_deref().map_or(Touched::Everything, Touched::Files)
    }

    /// Keep the lint set key once a full run found nothing blocking. A doc-only run skips the
    /// source lints, so it was not full.
    pub(crate) fn settle(&self, blocking: usize, doc_only: bool) {
        if self.files.is_some() || blocking > 0 || doc_only {
            return;
        }
        if let Some((path, key)) = &self.marker {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, key);
        }
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
