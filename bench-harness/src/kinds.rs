//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! How each bench kind's history is read.
//!
//! A run's summary rows record the code they measured in their `source`
//! column. A snapshot's is the id of its lock, so its history is the runs of
//! that one lock on one machine class: the code is fixed, and a timing that
//! leaves its history says the machine, the toolchain or the harness moved,
//! which this calls drift. A tracking bench's is `-`, and its code is named
//! by the run's commit, so its history is the runs at clean commits on one
//! machine class: a timing that leaves it says the code moved, a regression.
//! A tracking run on a dirty tree is compared and shown, and never flagged,
//! since no commit names the code it ran.
//!
//! What a bench may build from, by kind, is the lint pack's
//! `a-bench-keeps-the-contract-of-its-kind`, which `mock bench run` asks
//! before it builds anything.

use std::path::Path;

use mockspace_bench_layout::{Kind, TRACKING_SOURCE, source_id};
use sha2::{Digest, Sha256};

use crate::error::BenchError;
use crate::history::{HistoryEntry, Regression, detect_regressions};

/// The refusal for a bench that declares no kind.
#[must_use]
pub fn no_kind_reason(bench: &str) -> String {
    format!(
        "bench `{bench}` declares no `kind`. Every bench is one of two: `kind = \"snapshot\"`, \
         where {}; or `kind = \"tracking\"`, where {}. Declare the one it is, or run `mock bench \
         migrate --kinds`, which writes the kind each bench's dependencies admit.",
        Kind::Snapshot.contract(),
        Kind::Tracking.contract()
    )
}

/// The `source` a run of this bench records: a snapshot's lock id, read from
/// the lock at `lock`, or [`TRACKING_SOURCE`]. A snapshot with no lock is
/// refused: a run of it could not say which code it measured.
pub fn source_of(bench: &str, kind: Kind, lock: Option<&Path>) -> Result<String, BenchError> {
    if kind == Kind::Tracking {
        return Ok(TRACKING_SOURCE.to_string());
    }
    let refuse = |why: String| {
        BenchError::InvalidConfig {
            reason: format!(
                "snapshot `{bench}` {why}. A snapshot is frozen by its lock, and a run of it records \
             the lock's id so its history never mixes two versions of its code. Run `mock bench \
             freeze {bench}` to write it."
            ),
        }
    };
    let lock = lock.ok_or_else(|| refuse("has no lock location".into()))?;
    let bytes =
        std::fs::read(lock).map_err(|_| refuse(format!("has no lock at {}", lock.display())))?;
    let hex: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(source_id(&hex))
}

/// Whether a recorded commit names the code it ran: present, and clean.
fn names_its_code(commit: &str) -> bool {
    let c = commit.trim();
    !c.is_empty() && c != "unknown" && !c.ends_with("-dirty")
}

/// The earlier rows a run of `kind` is compared with, out of `historical` (one
/// cell on one machine class): a snapshot's rows of the same source, a
/// tracking bench's rows at clean commits.
#[must_use]
pub fn comparable(kind: Kind, source: &str, historical: &[HistoryEntry]) -> Vec<HistoryEntry> {
    historical
        .iter()
        .filter(|h| {
            match kind {
                Kind::Snapshot => !source.is_empty() && h.source == source,
                Kind::Tracking => names_its_code(&h.git_commit),
            }
        })
        .cloned()
        .collect()
}

/// A run's standing against its history, read by its kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    /// One row per variant that has comparable history.
    pub rows:  Vec<Regression>,
    /// What a flagged row is called: [`Kind::moved_label`].
    pub label: &'static str,
    /// Why nothing is flagged though rows moved, where that is so.
    pub note:  Option<String>,
}

/// Compare a run's rows, `current`, with the history of its cell on its machine
/// class, by the bench's kind.
#[must_use]
pub fn standing(
    kind: Kind,
    source: &str,
    current: &[HistoryEntry],
    historical: &[HistoryEntry],
) -> Standing {
    let mut rows = detect_regressions(current, &comparable(kind, source, historical));
    let dirty = kind == Kind::Tracking && current.iter().any(|c| !names_its_code(&c.git_commit));
    let note = if dirty && rows.iter().any(|r| r.flagged) {
        for r in &mut rows {
            r.flagged = false;
        }
        Some(
            "the run was taken on a tree with changes no commit names, so a move against \
             history is shown and not flagged; commit, then run again"
                .to_string(),
        )
    } else {
        None
    };
    Standing {
        rows,
        label: kind.moved_label(),
        note,
    }
}

/// The paragraph a run's report opens its methodology with: which kind of
/// bench it is, what code it measured, and how its numbers are to be read.
#[must_use]
pub fn methodology(kind: Kind, source: &str, commit: &str) -> String {
    match kind {
        Kind::Snapshot => {
            format!(
                "Kind: snapshot, source `{source}`. The code measured is the reference frozen by \
                 the bench's lock and is the same in every run of this source, so a difference \
                 against earlier runs on this machine class is drift of the machine, the \
                 toolchain or the harness, not of the code. Compare arms within a run freely; \
                 compare runs only of one source."
            )
        },
        Kind::Tracking => {
            format!(
                "Kind: tracking, at `{commit}`. The arms build against living code, so this run \
                 measures the code as it stood at that commit, and a difference against earlier \
                 runs on this machine class at clean commits is the code's trend. A run on a \
                 dirty tree is shown against history and never flagged."
            )
        },
    }
}

/// What code a run measured, read off its summary rows by their kind: `,
/// snapshot <source>` or `, tracking at <commit>`, and nothing for rows
/// written before the `source` column, which say neither. The index and the
/// generated bench docs both name a run's code this way.
#[must_use]
pub fn measured(rows: &[HistoryEntry]) -> String {
    match rows.first() {
        Some(e) if e.source == TRACKING_SOURCE => format!(", tracking at `{}`", e.git_commit),
        Some(e) if !e.source.is_empty() => format!(", snapshot `{}`", e.source),
        _ => String::new(),
    }
}

/// `report` with `paragraph` set after its title line, so the first thing a
/// reader of a run's report meets after the title is what kind of bench it is.
#[must_use]
pub fn with_kind_paragraph(report: &str, paragraph: &str) -> String {
    match report.split_once('\n') {
        Some((title, rest)) => format!("{title}\n\n{paragraph}\n{rest}"),
        None => format!("{report}\n\n{paragraph}\n"),
    }
}

#[cfg(test)]
#[path = "kinds_tests.rs"]
mod tests;
