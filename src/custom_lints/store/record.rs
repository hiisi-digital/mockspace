//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a run leaves beside the generated crate, and what a later run reads back: the owner, the
//! pointer, the tool names registered twice, and the stamp of last use.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{OWNER, POINTER, USED};

/// Record which worktree last wrote the shared crate, and where, so `mock test`
/// in a worktree can tell the crate is its own.
///
/// A build cut to one tool (`scope`) records its owner, which the reaper reads, and never the pointer: `mock
/// test` takes the lints it runs from the pointer, and a crate of one tool has none, so a pointer moved to it
/// is a green test run over no lints.
pub(crate) fn record_owner(
    gen_dir: &Path,
    mock_dir: &Path,
    scope: Option<&str>,
) -> Result<(), String> {
    crate::custom_lints::write_if_changed(&gen_dir.join(OWNER), &mock_dir.display().to_string())?;
    if scope.is_some() {
        return Ok(());
    }
    let local = crate::build_dir::ensure_under_target(mock_dir, &[crate::build_dir::LINT_CRATE]);
    crate::custom_lints::write_if_changed(&local.join(POINTER), &gen_dir.display().to_string())
}

/// The tool names more than one source registers, as the last whole build saw them, one per line.
const DUPLICATES: &str = "duplicate-tools";

/// Record, beside the pointer, which tool names the last whole build found registered more than once. A
/// run of one tool does not build the packs, so it cannot ask them; it reads this instead.
pub(crate) fn record_duplicates(mock_dir: &Path, names: &[String]) -> Result<(), String> {
    let local = crate::build_dir::ensure_under_target(mock_dir, &[crate::build_dir::LINT_CRATE]);
    crate::custom_lints::write_if_changed(&local.join(DUPLICATES), &names.join("\n"))
}

/// What the last whole build recorded: `None` when none has (or the file is gone), `Some(empty)` when it
/// found no duplicate.
pub(crate) fn duplicates_of(mock_dir: &Path) -> Option<Vec<String>> {
    read_record(&crate::build_dir::lint_crate_dir(mock_dir).join(DUPLICATES))
}

fn read_record(file: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(file).ok()?;
    Some(text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// The record a run of one tool reads: this worktree's own, else the newest one a sibling worktree of the
/// same clone left at the same mock directory.
///
/// A worktree made for one errand (a ticket seat, a helper's clone) has never run a whole build, and with
/// no record of its own its first run of one tool took the whole build, every lint and every tool in
/// release, which in a helper container ran past an hour. The record is the last whole build's and was
/// never tied to the revisions this run builds against, so a sibling's is the same kind of answer. A
/// collision only this worktree's own tools introduce is refused by its next whole build.
pub(crate) fn duplicates_for_run(repo_root: &Path, mock_dir: &Path) -> Option<Vec<String>> {
    if let Some(own) = duplicates_of(mock_dir) {
        return Some(own);
    }
    let rel = mock_dir.strip_prefix(repo_root).ok()?;
    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let newest = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(|w| crate::build_dir::lint_crate_dir(&Path::new(w).join(rel)).join(DUPLICATES))
        .filter_map(|f| Some((std::fs::metadata(&f).ok()?.modified().ok()?, f)))
        .max_by_key(|(at, _)| *at)?;
    read_record(&newest.1)
}
/// What a run of one tool does, given the record of the last whole build.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScopedPlan {
    /// The tool's name is registered twice: refused, as every whole run refuses it.
    Refuse(Vec<String>),
    /// Nothing recorded yet: one whole build, which writes the record.
    Whole,
    /// Recorded and clean for this tool: build that tool alone.
    OneTool,
}

pub(crate) fn plan_scoped(recorded: Option<Vec<String>>, tool: &str) -> ScopedPlan {
    match recorded {
        None => ScopedPlan::Whole,
        Some(d) if d.iter().any(|n| n == tool) => ScopedPlan::Refuse(d),
        Some(_) => ScopedPlan::OneTool,
    }
}

/// Stamp the directory as used now. Written on every run, unlike the owner,
/// because its modification time is the whole of what it records.
pub(crate) fn touch_used(gen_dir: &Path) -> Result<(), String> {
    let path = gen_dir.join(USED);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    std::fs::write(&path, now.to_string())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Where the generated crate stands for one worktree.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Generated {
    /// This worktree never wrote one, or what it wrote has been removed.
    Absent,
    /// The crate is at this directory and this worktree wrote it last.
    Yours(PathBuf),
    /// The crate is at this directory and another worktree has rewritten it
    /// since, to its own lint sources. Testing it from here would test theirs.
    Taken {
        dir:   PathBuf,
        owner: String,
    },
}

/// Which of the three the crate this worktree last wrote is in now.
pub(crate) fn generated_dir_of(mock_dir: &Path) -> Generated {
    let pointer = crate::build_dir::lint_crate_dir(mock_dir).join(POINTER);
    let Ok(dir) = std::fs::read_to_string(pointer) else {
        return Generated::Absent;
    };
    let dir = PathBuf::from(dir.trim());
    let Ok(owner) = std::fs::read_to_string(dir.join(OWNER)) else {
        return Generated::Absent;
    };
    if !dir.join("Cargo.toml").is_file() {
        return Generated::Absent;
    }
    let owner = owner.trim().to_string();
    if owner == mock_dir.display().to_string() {
        Generated::Yours(dir)
    } else {
        Generated::Taken {
            dir,
            owner,
        }
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
