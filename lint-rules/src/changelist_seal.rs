//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Repo lint: a locked changelist carries its text, and a round closes with
//! nothing left unlocked.
//!
//! The shape it refuses is one mistake seen at three moments. A changelist is
//! written by hand while a second one is opened from a bare template, the
//! template is the one that gets locked, and the text stays unlocked beside it.
//! Every other gate passes, because a lock exists and the phase reads right, so
//! the round closes with a lock saying nothing and the claims it was for
//! sitting in a file nothing froze.
//!
//! - In the active round: a locked changelist holding nothing but a bare title,
//!   and an unlocked changelist beside a locked one of the same kind.
//! - In a closed round directory: an unlocked changelist, or a locked one
//!   holding nothing but a bare title, where the file is not committed at that
//!   path yet. That is a close carrying either of the above into history.
//!
//! A closed round directory is one named by a `YYYYMMDDHHMM` stamp, or by one
//! with a `-N` suffix, which is what a close writes when two rounds share a
//! minute. An abandoned round's directory is not one: an abandoned round may
//! hold unlocked changelists, since abandoning is exactly not finishing.
//!
//! Files already committed inside a closed round are not read. They are frozen,
//! so a finding there could never be repaired and would block every commit
//! after it, and the repair for one is forward, in the next round. The active
//! round is always read, since everything in it can still be unlocked,
//! deprecated or rewritten.
//!
//! What is committed is read off `HEAD`, which a close committed through a hook
//! passes through first. A close or a lock that commits without the hooks is
//! why `lock` and `close` run [`active_round_findings`] and
//! [`says_nothing`] themselves before they move anything.
//!
//! Severity: Error (blocks commit, push, and build).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, ClKind, ClStatus, ParsedChangelist};
use crate::{Lint, LintError, RepoContext, RepoLint, Severity};

const LINT_NAME: &str = "changelist-seal";

pub struct ChangelistSeal;

impl Lint for ChangelistSeal {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    /// The design-round gate. Blocking is the tool.
    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistSeal {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let design_rounds = ctx.mock_dir.join("design_rounds");
        let mut findings = active_round_findings(&design_rounds);
        let committed = committed_files(&design_rounds);
        for round in closed_round_dirs(&design_rounds) {
            let none = BTreeSet::new();
            let held = committed.get(&round).unwrap_or(&none);
            closed_round_findings(&design_rounds, &round, held, &mut findings);
        }
        findings
            .into_iter()
            .map(|msg| LintError::error("workspace".to_string(), 0, LINT_NAME, msg))
            .collect()
    }
}

/// What is wrong with the active round's changelists, one message each.
///
/// Public because `close` refuses on it before moving anything, which is the
/// one place a close that commits without the hooks can be stopped.
pub fn active_round_findings(design_rounds: &Path) -> Vec<String> {
    let cls = changelist_helpers::find_changelists(design_rounds);
    let mut findings = Vec::new();
    for cl in &cls {
        if cl.status == ClStatus::Locked && says_nothing(&design_rounds.join(&cl.filename)) {
            findings.push(empty_lock(&format!("design_rounds/{}", cl.filename)));
        }
    }
    for kind in [ClKind::Doc, ClKind::Src] {
        let of = |status: ClStatus| -> Vec<&ParsedChangelist> {
            cls.iter()
                .filter(|cl| cl.kind == kind && cl.status == status)
                .collect()
        };
        let locked = of(ClStatus::Locked);
        let Some(lock) = locked.first() else { continue };
        for active in of(ClStatus::Active) {
            findings.push(format!(
                "`design_rounds/{active}` is unlocked beside `design_rounds/{lock}`, \
                 a locked changelist of the same kind, so one of them is not this \
                 round's record. `cargo mock unlock` deprecates the src changelists \
                 and unlocks the doc changelist; carry the text into the changelist \
                 of that kind which is left or opened next, and lock back up to \
                 where the round was.",
                active = active.filename,
                lock = lock.filename,
            ));
        }
    }
    findings
}

fn closed_round_findings(
    design_rounds: &Path,
    round: &str,
    committed: &BTreeSet<String>,
    findings: &mut Vec<String>,
) {
    let dir = design_rounds.join(round);
    for cl in changelist_helpers::find_changelists(&dir) {
        if committed.contains(&cl.filename) {
            continue;
        }
        let rel = format!("design_rounds/{round}/{}", cl.filename);
        match cl.status {
            ClStatus::Active => {
                findings.push(format!(
                    "`{rel}` is an unlocked changelist inside a closed round, so its text \
                 was never frozen. The close is not committed yet: move the round's \
                 files back into `design_rounds/`, lock or deprecate it there, and \
                 close again."
                ))
            },
            ClStatus::Locked if says_nothing(&dir.join(&cl.filename)) => {
                findings.push(empty_lock(&rel))
            },
            ClStatus::Locked | ClStatus::Deprecated => {},
        }
    }
}

fn empty_lock(rel: &str) -> String {
    format!(
        "`{rel}` is locked holding nothing but a bare title. A lock freezes what \
         the changelist says, and this one says nothing; the text it was for is \
         usually in an unlocked sibling. `cargo mock unlock` deprecates the src \
         changelists and unlocks the doc changelist; write what this one should \
         have said into the changelist of its kind that is left or opened next, \
         and lock back up to where the round was."
    )
}

/// Whether a changelist is a bare title and nothing else.
///
/// Public because `lock` refuses to lock one, which is where the empty lock
/// this lint exists for is made.
///
/// An unreadable file is not called empty: this judges what a file says, and a
/// file that could not be read has not been shown to say nothing.
pub fn says_nothing(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(text) => is_bare_title(&text),
        Err(_) => false,
    }
}

/// At most one line that is not blank, that one a heading, and the heading
/// naming nothing after its kind.
///
/// A second heading is content, since a `## CHANGE:` heading is a claim by
/// itself. So is a subject after a colon: `# doc changelist: none` is a round
/// declaring it has no doc edits, which is a statement, where `# src changelist`
/// is a template nobody wrote into.
fn is_bare_title(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (None, _) => true,
        (Some(first), None) => {
            let names_something = first
                .split_once(':')
                .is_some_and(|(_, subject)| !subject.trim().is_empty());
            first.starts_with('#') && !names_something
        },
        (Some(_), Some(_)) => false,
    }
}

/// Closed rounds: the subdirectories of `design_rounds/` named by a round
/// stamp, sorted.
fn closed_round_dirs(design_rounds: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(design_rounds) else {
        return Vec::new();
    };
    let mut rounds: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| is_round_dir(name))
        .collect();
    rounds.sort();
    rounds
}

/// `YYYYMMDDHHMM`, or that with `-N` after it for a close that shared its
/// minute with an earlier one. Not `-abandoned`, nor anything else.
fn is_round_dir(name: &str) -> bool {
    let (stamp, suffix) = match name.split_once('-') {
        Some((stamp, suffix)) => (stamp, Some(suffix)),
        None => (name, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    stamp.len() == 12 && digits(stamp) && suffix.is_none_or(digits)
}

/// The files `HEAD` holds directly inside each round directory, by round.
///
/// One `git` call for the whole tree rather than one per round: a repository
/// carrying a hundred and fifty rounds runs this at every gate.
///
/// Empty where git cannot answer, a repository with no commit yet among them,
/// and then every closed round is judged, since none of it has been shown to be
/// history.
fn committed_files(design_rounds: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_round: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let Ok(output) = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD", "--", "."])
        .current_dir(design_rounds)
        .output()
    else {
        return by_round;
    };
    if !output.status.success() {
        return by_round;
    }
    for path in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((round, file)) = path.split_once('/')
            && !file.contains('/')
        {
            by_round
                .entry(round.to_string())
                .or_default()
                .insert(file.to_string());
        }
    }
    by_round
}

#[cfg(test)]
#[path = "changelist_seal_tests.rs"]
mod tests;
