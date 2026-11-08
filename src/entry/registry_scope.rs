//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which registry findings gate a run: the one function that decides it.
//!
//! A check gates only on a commit that changes what it reads. A finding about a data row
//! carries the file the row lives in (`RegistryFinding::source`), so it blocks only the commit
//! that stages that file; on any other run, and on a commit that leaves the file alone, it is
//! reported as a warning. A fresh clone with large files absent, or a row retired while source
//! still cites it, then does not stop unrelated commits, and the error arrives with the commit
//! that next touches the file.
//!
//! A finding with no `source` is about the declarations (schema, namespace collisions, unknown
//! field types, duplicate identifiers) and not about one data row, so it gates as it always did.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::LintMode;
use crate::registry::RegistryFinding;

/// What the run knows about the files it is judging.
pub(crate) enum Changed {
    /// Not the commit gate. No file is being committed, so no row finding gates.
    NotCommit,
    /// The commit gate, with the files the commit stages, as absolute paths.
    Staged(Vec<PathBuf>),
    /// The commit gate, and git could not say what is staged. A check that could not run is not a
    /// check that passed, so every finding gates.
    Unknown,
}

impl Changed {
    /// The scope of a run: the staged files for the commit gate, nothing otherwise.
    pub(crate) fn of_run(mode: LintMode, repo_root: &Path) -> Self {
        if mode != LintMode::Commit {
            return Self::NotCommit;
        }
        match staged_paths(repo_root) {
            Some(paths) => Self::Staged(paths),
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

/// The only place the severity of a registry finding is decided, shared by the plain run and the
/// commit gate.
pub(crate) fn verdict(f: &RegistryFinding, changed: &Changed) -> Verdict {
    let Some(source) = &f.source else {
        return Verdict::Error;
    };
    match changed {
        Changed::NotCommit => Verdict::Warning,
        Changed::Unknown => Verdict::Error,
        Changed::Staged(paths) if paths.iter().any(|p| p == source) => Verdict::Error,
        Changed::Staged(_) => Verdict::Warning,
    }
}

/// The files the index changes against `HEAD`, absolute. `None` when git cannot say.
fn staged_paths(repo_root: &Path) -> Option<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["diff", "--cached", "--name-only", "-z", "--no-renames", "--relative"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| repo_root.join(p))
            .collect(),
    )
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
