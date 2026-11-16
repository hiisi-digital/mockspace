//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What each bench kind says a run measured.
//!
//! A run's summary rows record the code they measured in their `source`
//! column and the system that ran them in their `system` column. A
//! snapshot's source is the id of its lock, the same for every arm. A
//! tracking arm's is the digest `mock bench run` takes of every file the arm
//! built from, in the tree and out of it, and of the versions its other
//! dependencies resolved to, handed to the driver in
//! [`SOURCES_ENV`](mockspace_bench_layout::SOURCES_ENV). Both are read the
//! same way from there, along the axes of [`crate::axes`].
//!
//! What a bench may build from, by kind, is the lint pack's
//! `a-bench-keeps-the-contract-of-its-kind`, which `mock bench run` asks
//! before it builds anything.

use std::path::{Component, Path, PathBuf};

use mockspace_bench_layout::{Kind, TRACKING_SOURCE, source_id};
use sha2::{Digest, Sha256};

use crate::error::BenchError;
use crate::history::HistoryEntry;

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

/// The `source` a run of this bench records where no arm says otherwise: a
/// snapshot's lock id, read from the lock at `lock`, or [`TRACKING_SOURCE`],
/// which a tracking arm's own digest replaces ([`Sources::of`]). A snapshot
/// with no lock is refused: a run of it could not say which code it measured.
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
    Ok(source_id(&hex(&bytes)))
}

/// The sha256 of `bytes`, in hex.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `path` with `.` and `..` taken out lexically.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                out.pop();
            },
            other => out.push(other),
        }
    }
    out
}

/// The tracking arms' sources of one run, read from the file `mock bench run`
/// names in [`SOURCES_ENV`](mockspace_bench_layout::SOURCES_ENV): one
/// `<path prefix>\t<source>` line per arm, the prefix relative to the bench
/// tree and the start of the arm's built library path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    by_prefix: Vec<(PathBuf, String)>,
}

impl Sources {
    /// Parse the file's text.
    #[must_use]
    pub fn parse(text: &str) -> Sources {
        let mut by_prefix: Vec<(PathBuf, String)> = text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(p, s)| (normal(Path::new(p)), s.trim().to_string()))
            .collect();
        // The longest prefix first, so an arm nested under another's path is
        // found before it.
        by_prefix.sort_by_key(|(p, _)| std::cmp::Reverse(p.components().count()));
        Sources {
            by_prefix,
        }
    }

    /// The sources this process was handed, none where it was run by hand.
    #[must_use]
    pub fn from_env() -> Option<Sources> {
        let path = std::env::var(mockspace_bench_layout::SOURCES_ENV).ok()?;
        Some(Sources::parse(&std::fs::read_to_string(path).unwrap_or_default()))
    }

    /// The source a variant of a bench of `kind` records: the one `handed`
    /// names for its library at `path`, or the bench's own (`fallback`, the
    /// lock id of a snapshot). A tracking arm that a run handed sources to and
    /// that finds none of its own is refused rather than written as unknown,
    /// since its rows could then stand at no point.
    pub fn for_variant(
        handed: Option<&Sources>,
        kind: mockspace_bench_layout::Kind,
        root: &Path,
        path: &Path,
        fallback: &str,
    ) -> Result<String, String> {
        match (handed.and_then(|s| s.of(root, path)), handed, kind) {
            (Some(s), _, _) => Ok(s.to_string()),
            (None, Some(_), mockspace_bench_layout::Kind::Tracking) => Err(format!(
                "`mock bench run` handed no source for {}, so its rows could stand at no point",
                path.display()
            )),
            _ => Ok(fallback.to_string()),
        }
    }

    /// The source of the arm built at `path`, relative to `root` (the bench
    /// tree), where one was handed for it.
    #[must_use]
    pub fn of(&self, root: &Path, path: &Path) -> Option<&str> {
        let rel = normal(path.strip_prefix(root).unwrap_or(path));
        self.by_prefix
            .iter()
            .find(|(p, _)| rel.starts_with(p))
            .map(|(_, s)| s.as_str())
    }
}

/// The paragraph a run's report opens its methodology with: which kind of
/// bench it is, the system it ran on, what code it measured, and how its
/// numbers are read.
#[must_use]
pub fn methodology(kind: Kind, source: &str, commit: &str, system: &str) -> String {
    let reading = "Runs at one source on one system are one point, every run adding to it; a \
                   difference against that point is drift of the machine, the toolchain or the \
                   harness, a difference between a point and the one before it is the code's. \
                   Another system is another variant, never compared with this one.";
    match kind {
        Kind::Snapshot => {
            format!(
                "Kind: snapshot, source `{source}`, on `{system}`. The code measured is the \
                 reference frozen by the bench's lock. {reading}"
            )
        },
        Kind::Tracking => {
            format!(
                "Kind: tracking, at `{commit}`, on `{system}`. The arms build against living \
                 code, and each row's source is the digest of every file its arm built from and \
                 of the versions its dependencies resolved to. {reading}"
            )
        },
    }
}

/// What a run measured and where, read off its summary rows: `, on <system>
/// at <commit>`. The index and the generated bench docs both name a run this
/// way; the sources are in each row and in the trails.
#[must_use]
pub fn measured(rows: &[HistoryEntry]) -> String {
    match rows.first() {
        Some(e) if !e.system.is_empty() => format!(", on `{}` at `{}`", e.system, e.git_commit),
        Some(e) => format!(", at `{}`", e.git_commit),
        None => String::new(),
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
