//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which registry findings gate a run: the one function that decides it.
//!
//! A reference to a row (a row citing a retired or absent row, a template citing one) gates only
//! the run that changes the file holding the citation. The commit gate changes the staged files,
//! the push gate the files of the commits not yet on any remote, and any other run changes
//! nothing, so there the finding is a warning. A row can then be retired while source still cites
//! it, and the error arrives with the commit that next touches the citing file.
//!
//! The scope is a property of the finding, set where it is made (`RegistryFinding::gate`), and
//! nothing is inferred from whether it names a file. Every other finding (duplicate identifiers,
//! cycles, provenance, declarations, values) is `GateScope::Always` and gates every run.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::LintMode;
use crate::registry::{GateScope, RegistryFinding};

/// What the run knows about the files it is judging.
#[derive(Debug)]
pub(crate) enum Changed {
    /// Neither the commit nor the push gate. No change is being judged, so no row reference gates.
    NotCommit,
    /// The files the run changes, as absolute paths: staged for a commit, in the unpushed commits
    /// for a push.
    Files(Vec<PathBuf>),
    /// A commit or push run, and git could not say what changes. A check that could not run is
    /// not a check that passed, so every finding gates.
    Unknown,
}

impl Changed {
    /// The scope of a run, for the index the hook that started it was given.
    pub(crate) fn of_run(mode: LintMode, repo_root: &Path) -> Self {
        Self::of_run_with_index(
            mode,
            repo_root,
            super::env::hook_index_file().map(Path::new),
        )
    }

    /// The scope of a run. `index` is the `GIT_INDEX_FILE` of the hook: under `git commit -a` or
    /// `git commit <path>` git commits a temporary index and leaves `.git/index` as it was, so the
    /// commit's files are only in the one it hands the hook. A relative one is relative to the top
    /// of the work tree.
    pub(crate) fn of_run_with_index(
        mode: LintMode,
        repo_root: &Path,
        index: Option<&Path>,
    ) -> Self {
        let paths = match mode {
            LintMode::Commit => {
                git_paths(
                    repo_root,
                    &["diff", "--cached", "--name-only", "-z", "--no-renames", "--relative"],
                    index.map(|i| repo_root.join(i)),
                )
            },
            // A merge commit lists no file unless asked, so the first parent's diff is asked for:
            // what the merge brought in.
            LintMode::Push => {
                git_paths(
                    repo_root,
                    &[
                        "log",
                        "--name-only",
                        "--format=",
                        "-z",
                        "--no-renames",
                        "--relative",
                        "--diff-merges=first-parent",
                        "HEAD",
                        "--not",
                        "--remotes",
                    ],
                    None,
                )
            },
            LintMode::Build => return Self::NotCommit,
        };
        match paths {
            Some(paths) => Self::Files(paths),
            None => Self::Unknown,
        }
    }
}

/// What a finding does to the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Error,
    Warning,
}

/// The only place the severity of a registry finding is decided, shared by the plain run, the
/// commit gate and the push gate.
pub(crate) fn verdict(f: &RegistryFinding, changed: &Changed) -> Verdict {
    let GateScope::WhenChanged(file) = &f.gate else {
        return Verdict::Error;
    };
    match changed {
        Changed::NotCommit => Verdict::Warning,
        Changed::Unknown => Verdict::Error,
        Changed::Files(paths) if paths.iter().any(|p| p == file) => Verdict::Error,
        Changed::Files(_) => Verdict::Warning,
    }
}

/// The files a git listing names, absolute. `None` when git cannot say.
fn git_paths(repo_root: &Path, args: &[&str], index: Option<PathBuf>) -> Option<Vec<PathBuf>> {
    let mut git = Command::new("git");
    git.arg("-C").arg(repo_root).args(args);
    if let Some(index) = index {
        git.env("GIT_INDEX_FILE", index);
    }
    let out = git.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut paths: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .map(|p| p.trim_matches('\n'))
        .filter(|p| !p.is_empty())
        .map(|p| repo_root.join(p))
        .collect();
    paths.sort();
    paths.dedup();
    Some(paths)
}

/// Prints each finding at the level its verdict gives and returns `(gated, demoted)`.
pub(crate) fn report(
    findings: impl IntoIterator<Item = RegistryFinding>,
    changed: &Changed,
) -> (usize, usize) {
    let (mut gated, mut demoted) = (0, 0);
    for f in findings {
        match verdict(&f, changed) {
            Verdict::Error => {
                gated += 1;
                eprintln!("  ERROR [{}]: {}", f.kind, f.message);
            },
            Verdict::Warning => {
                demoted += 1;
                eprintln!("  warning [{}]: {}", f.kind, f.message);
            },
        }
    }
    (gated, demoted)
}
