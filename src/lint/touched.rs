//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a commit gate run changes, for the lints that declare the paths they read.
//!
//! A lint declaring [`Reach::Paths`](mockspace_lint_rules::Reach::Paths) is run only by a commit
//! changing a file its globs match (the lint's own source and the tooling it runs are listed in
//! those globs by the lint). This module reduces the run's [`Changed`] to that list, and decides
//! when the list is not enough: **a commit changing the lint set runs every lint.** The set is the
//! configuration (`mockspace.toml`, wherever it is), the mock directory's own `lints/` and `tools/`
//! and its manifest, lockfile and toolchain pin, which is where a pack and the engine it builds
//! against are named. The notion of what changed is `registry_scope::Changed`, the one the row
//! reference gate uses.
//!
//! Only the commit gate narrows. A push, a plain run and a commit git could not list run every lint.

use std::path::Path;

use mockspace_lint_rules::LintMode;

use crate::entry::registry_scope::Changed;

/// The repository-relative files a commit changes, or `None` where every lint runs.
pub(crate) fn of_run(mode: LintMode, repo_root: &Path, mock_dir: &Path) -> Option<Vec<String>> {
    if mode != LintMode::Commit {
        return None;
    }
    reduce(&Changed::of_run(mode, repo_root), repo_root, mock_dir)
}

/// [`of_run`] for a `changed` already known.
pub(crate) fn reduce(changed: &Changed, repo_root: &Path, mock_dir: &Path) -> Option<Vec<String>> {
    let Changed::Files(paths) = changed else {
        return None;
    };
    let mock = mock_dir.strip_prefix(repo_root).unwrap_or(Path::new(""));
    let set_dirs = [mock.join("lints"), mock.join("tools")];
    let set_files = [mock.join("Cargo.toml"), mock.join("Cargo.lock"), mock.join("rust-toolchain.toml")];
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
