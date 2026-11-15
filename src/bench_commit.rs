//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit a bench run measured, and whether what it was built from was
//! dirty.
//!
//! A timing is traceable to a commit only where nothing it was built from
//! differed from that commit. The mark used to read `git status --porcelain`
//! over the whole worktree, so a run's own results, a generated file a
//! repository forgot to ignore, or two tracked paths that differ only in case
//! on a case-insensitive volume each marked every later timing dirty although
//! no code had changed.
//!
//! Here the mark is asked of the bench tree's build inputs alone, the set
//! `mock test` already keys the bench tree on
//! ([`crate::entry::test_benches::inputs`]): the files under the bench tree
//! less every bench output, every local crate a bench manifest reaches by
//! `path`, and the workspace's manifests, lockfiles, toolchain pin and cargo
//! configuration. Only tracked changes count: a file git does not track is not
//! part of the commit. `mock bench run` hands the answer to the driver in
//! [`COMMIT_ENV`], and the run's meta and summary record it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) use mockspace_bench_harness::env::COMMIT_ENV;

/// The short commit of the repository at `repo_root`, with `-dirty` when a
/// tracked build input of the bench tree at `bench_dir` differs from it.
/// `None` outside a git repository, where the driver records none.
pub(crate) fn commit_marked(repo_root: &Path, mock_dir: &Path, bench_dir: &Path) -> Option<String> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout)
    };
    let head = String::from_utf8(git(&["rev-parse", "--short", "HEAD"])?).ok()?;
    let head = head.trim().to_string();
    let status = git(&["status", "--porcelain=v1", "-z", "--untracked-files=no"])?;
    let dirty = match crate::entry::test_benches::inputs(repo_root, mock_dir, bench_dir) {
        Ok((files, _)) => {
            let inputs: BTreeSet<PathBuf> = files.into_iter().collect();
            changed(&status, repo_root).iter().any(|p| inputs.contains(p))
        },
        // Inputs that cannot be listed are not a clean tree: any tracked
        // change marks it, as the driver run by hand would.
        Err(_) => !changed(&status, repo_root).is_empty(),
    };
    Some(if dirty { format!("{head}-dirty") } else { head })
}

/// Every path a `git status --porcelain=v1 -z` answer names, absolute under
/// `repo_root`, both sides of a rename or a copy included.
pub(crate) fn changed(porcelain_z: &[u8], repo_root: &Path) -> Vec<PathBuf> {
    let mut fields = porcelain_z.split(|b| *b == 0).filter(|f| !f.is_empty());
    let mut out = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = (&entry[.. 2], &entry[3 ..]);
        out.push(repo_root.join(String::from_utf8_lossy(path).as_ref()));
        if xy.contains(&b'R') || xy.contains(&b'C') {
            if let Some(from) = fields.next() {
                out.push(repo_root.join(String::from_utf8_lossy(from).as_ref()));
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "bench_commit_tests.rs"]
mod tests;
