//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Repo lint: a locked changelist carries its text, and a round closes with
//! nothing left unlocked.
//!
//! The shape it refuses is one mistake seen at three moments. A changelist is
//! written by hand while a second one is stamped from the template, the
//! template is the one that gets locked, and the text stays unlocked beside it.
//! Every other gate passes, because a lock exists and the phase reads right, so
//! the round closes with a lock saying nothing and the claims it was for
//! sitting in a file nothing froze.
//!
//! - In the active round: a locked changelist holding nothing but its title,
//!   and an unlocked changelist beside a locked one of the same kind.
//! - In a closed round directory: an unlocked changelist, or a locked one
//!   holding nothing but its title, where the file is not yet committed at that
//!   path. That is the close carrying either of the above into history.
//!
//! Files already committed inside a closed round are not read. They are frozen,
//! so a finding there could never be repaired and would block every commit
//! after it, and the repair for one is forward, in the next round. The active
//! round is always read, since everything in it can still be unlocked,
//! deprecated or edited.
//!
//! Severity: Error (blocks commit, push, and build).

use std::collections::BTreeSet;
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
        let mut errors = Vec::new();
        check_active_round(&design_rounds, &mut errors);
        for round in closed_round_dirs(&design_rounds) {
            check_closed_round(&design_rounds, &round, &mut errors);
        }
        errors
    }
}

fn check_active_round(design_rounds: &Path, errors: &mut Vec<LintError>) {
    let cls = changelist_helpers::find_changelists(design_rounds);
    for cl in &cls {
        if cl.status == ClStatus::Locked && says_nothing(&design_rounds.join(&cl.filename)) {
            errors.push(empty_lock(&format!("design_rounds/{}", cl.filename)));
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
            errors.push(error(format!(
                "`design_rounds/{active}` is unlocked beside `design_rounds/{lock}`, \
                 a locked changelist of the same kind. One of them is not this round's \
                 record: if the text is in the unlocked one, unlock the other, move \
                 the text into it and lock again; if it is a leftover, deprecate it.",
                active = active.filename,
                lock = lock.filename,
            )));
        }
    }
}

fn check_closed_round(design_rounds: &Path, round: &str, errors: &mut Vec<LintError>) {
    let dir = design_rounds.join(round);
    let committed = committed_files(design_rounds, round);
    for cl in changelist_helpers::find_changelists(&dir) {
        if committed.contains(&cl.filename) {
            continue;
        }
        let rel = format!("design_rounds/{round}/{}", cl.filename);
        match cl.status {
            ClStatus::Active => errors.push(error(format!(
                "`{rel}` is an unlocked changelist inside a closed round. A round \
                 closes with every changelist locked or deprecated, so the text in \
                 this one was never frozen. Reopen the round, lock or deprecate it, \
                 and close again."
            ))),
            ClStatus::Locked if says_nothing(&dir.join(&cl.filename)) => {
                errors.push(empty_lock(&rel))
            }
            ClStatus::Locked | ClStatus::Deprecated => {}
        }
    }
}

fn empty_lock(rel: &str) -> LintError {
    error(format!(
        "`{rel}` is locked holding nothing but its title. A lock freezes what \
         the changelist says, and this one says nothing; the text it was for is \
         usually in an unlocked sibling. Unlock it, write the changelist, and \
         lock again."
    ))
}

fn error(msg: String) -> LintError {
    LintError::error("workspace".to_string(), 0, LINT_NAME, msg)
}

/// Whether a changelist is its title and nothing else, which is what the
/// subcommands stamp when they open one.
///
/// An unreadable file is not called empty: this lint judges what a file says,
/// and a file it could not read has not been shown to say nothing.
fn says_nothing(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(text) => is_title_only(&text),
        Err(_) => false,
    }
}

/// At most one line that is not blank, and that one a heading. A second
/// heading is content, since a `## CHANGE:` heading is a claim by itself.
fn is_title_only(text: &str) -> bool {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (None, _) => true,
        (Some(first), None) => first.starts_with('#'),
        (Some(_), Some(_)) => false,
    }
}

/// Closed rounds: the subdirectories of `design_rounds/` named by a
/// `YYYYMMDDHHMM` stamp, which is what `close` writes.
fn closed_round_dirs(design_rounds: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(design_rounds) else {
        return Vec::new();
    };
    let mut rounds: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| is_round_stamp(name))
        .collect();
    rounds.sort();
    rounds
}

fn is_round_stamp(name: &str) -> bool {
    name.len() == 12 && name.bytes().all(|b| b.is_ascii_digit())
}

/// The filenames `HEAD` holds directly inside `design_rounds/<round>/`.
///
/// Empty where git cannot answer, a repository with no commit yet among them,
/// and then every changelist in the round is judged, since none of it has been
/// shown to be history.
fn committed_files(design_rounds: &Path, round: &str) -> BTreeSet<String> {
    let Ok(output) = Command::new("git")
        .args(["ls-tree", "--name-only", "HEAD", "--"])
        .arg(format!("{round}/"))
        .current_dir(design_rounds)
        .output()
    else {
        return BTreeSet::new();
    };
    if !output.status.success() {
        return BTreeSet::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|path| path.strip_prefix(&format!("{round}/")).map(str::to_string))
        .collect()
}

#[cfg(test)]
#[path = "changelist_seal_tests.rs"]
mod tests;
