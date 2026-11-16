//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The axes a bench's history is split on, and what is read along them.
//!
//! A summary row stands at a point: its variant (the arm), the system that ran
//! it ([`crate::system`]) and the source it measured (a snapshot's lock id, a
//! tracking arm's digest of what it built from). Each is an [`Axis`],
//! and every reading of history goes through them the same way for both
//! kinds of bench:
//!
//! - rows are compared only within one variant on one system; another system
//!   is another variant, shown beside it, never pooled with it;
//! - rows of one source are one point, and every run at that point adds to it,
//!   whatever commit it was taken at, so a timing does not swing with commits
//!   that left the code alone;
//! - a run at the point the history already has is checked for drift, which
//!   is the machine, the toolchain or the harness moving, since the code did
//!   not; a run at a new point is compared with the point before it, and the
//!   step is a regression or a speed-up of the code;
//! - a source that names no code ([`known`]) is no point: its rows are kept
//!   and counted, and never read against or as history;
//! - a point is every row of its source ([`point_of`]), for the run's label
//!   and the trail alike, so the two never disagree on the same rows.
//!
//! Grouping is for reading only. Nothing here drops, merges or skips a row:
//! every run is written as its own run directory and read back whole.

use std::collections::BTreeMap;

use crate::history::HistoryEntry;

/// One axis of a row's point.
pub trait Axis {
    /// What the axis is called in a view.
    const NAME: &'static str;
    /// The row's value on the axis.
    fn of(e: &HistoryEntry) -> &str;
}

/// The arm, by its exported name.
pub struct Variant;
/// The system that ran the row: machine class and target.
pub struct System;
/// The code the row measured.
pub struct Source;

impl Axis for Variant {
    const NAME: &'static str = "variant";

    fn of(e: &HistoryEntry) -> &str {
        &e.variant
    }
}

impl Axis for System {
    const NAME: &'static str = "system";

    fn of(e: &HistoryEntry) -> &str {
        &e.system
    }
}

impl Axis for Source {
    const NAME: &'static str = "source";

    fn of(e: &HistoryEntry) -> &str {
        &e.source
    }
}

/// The rows whose value on `A` is `value`.
#[must_use]
pub fn on<A: Axis>(rows: &[HistoryEntry], value: &str) -> Vec<HistoryEntry> {
    rows.iter().filter(|e| A::of(e) == value).cloned().collect()
}

/// The rows split by their value on `A`, each value once, in the order it
/// first ran.
#[must_use]
pub fn split<A: Axis>(rows: &[HistoryEntry]) -> Vec<(String, Vec<HistoryEntry>)> {
    let mut groups: BTreeMap<String, Vec<HistoryEntry>> = BTreeMap::new();
    for e in rows {
        groups
            .entry(A::of(e).to_string())
            .or_default()
            .push(e.clone());
    }
    let mut out: Vec<(String, Vec<HistoryEntry>)> = groups.into_iter().collect();
    out.sort_by_key(|(v, rows)| {
        (
            rows.iter().map(|e| e.timestamp).min().unwrap_or(0),
            v.clone(),
        )
    });
    out
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    match v.len() {
        0 => 0.0,
        n if n % 2 == 0 => (v[n / 2 - 1] + v[n / 2]) / 2.0,
        n => v[n / 2],
    }
}

/// One source point of a variant on a system: every row of that source.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// The source.
    pub source:    String,
    /// How many rows, one per run, stand at it.
    pub runs:      usize,
    /// The median of their medians.
    pub median_ns: f64,
    /// The median of their interval bounds.
    pub ci_lo_ns:  f64,
    pub ci_hi_ns:  f64,
}

/// Which way a step between two points went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// The later point's interval lies wholly above the earlier one's.
    Slower,
    /// Wholly below.
    Faster,
    /// They overlap.
    Level,
}

/// Whether a source names code. A row written before the `source` column
/// (`""`) or by a driver no source was handed to
/// ([`TRACKING_SOURCE`](mockspace_bench_layout::TRACKING_SOURCE), `-`) names
/// none: it is never a point, never a step, and never a history anything is
/// read against. Such rows are kept, written and counted, and only left out
/// of the reading.
#[must_use]
pub fn known(source: &str) -> bool {
    !source.is_empty() && source != mockspace_bench_layout::TRACKING_SOURCE
}

/// The point of `source` among `rows`: every one of them at that source. The
/// one definition of a point, which the trail and the run's standing share.
#[must_use]
pub fn point_of(rows: &[HistoryEntry], source: &str) -> Point {
    let at = on::<Source>(rows, source);
    Point {
        source:    source.to_string(),
        runs:      at.len(),
        median_ns: median(at.iter().map(|e| e.median_ns).collect()),
        ci_lo_ns:  median(at.iter().map(|e| e.ci_lo_ns).collect()),
        ci_hi_ns:  median(at.iter().map(|e| e.ci_hi_ns).collect()),
    }
}

/// The source points of one variant on one system in the order the runs
/// visited them, a source returning to after another counting again where
/// it returns, each with every row of its source ([`point_of`]). Rows whose
/// source is not [`known`] are not points and are passed over.
#[must_use]
pub fn trail(rows: &[HistoryEntry]) -> Vec<Point> {
    let mut by_time: Vec<&HistoryEntry> = rows.iter().filter(|e| known(&e.source)).collect();
    by_time.sort_by_key(|e| e.timestamp);
    let mut order: Vec<&str> = Vec::new();
    for e in by_time {
        if order.last() != Some(&e.source.as_str()) {
            order.push(&e.source);
        }
    }
    order.into_iter().map(|s| point_of(rows, s)).collect()
}

/// How `to` stands against `from`.
#[must_use]
pub fn step(from: &Point, to: &Point) -> Move {
    if to.ci_lo_ns > from.ci_hi_ns {
        Move::Slower
    } else if to.ci_hi_ns < from.ci_lo_ns {
        Move::Faster
    } else {
        Move::Level
    }
}

/// A run's timing that left its history, by what moved.
pub const DRIFT: &str = "DRIFT";
/// A new source point slower than the one before it.
pub const REGRESSION: &str = "REGRESSION";
/// A new source point faster than the one before it.
pub const FASTER: &str = "FASTER";

/// One finding of a run against its history.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The variant.
    pub variant: String,
    /// [`DRIFT`], [`REGRESSION`] or [`FASTER`].
    pub label:   &'static str,
    /// Change against what it was compared with, in percent.
    pub pct:     f64,
    /// The source it was compared with.
    pub against: String,
}

impl Finding {
    /// The operator's one line.
    #[must_use]
    pub fn render(&self, bench: &str) -> String {
        format!(
            "{}: {bench} {} {:+.1}% against source `{}`",
            self.label, self.variant, self.pct, self.against
        )
    }
}

/// Read a run's rows, `current`, against the history of its cell: each row
/// against the earlier rows of its own variant and system whose source is
/// [`known`], by the same points the trail reads. Where the latest of them is
/// of the row's own source, the run adds to that point, and its row against
/// the whole point is [`DRIFT`] where they part; where it is of another, the
/// run starts a new point, and the row against the whole point before is
/// [`REGRESSION`] or [`FASTER`]. A row whose own source is unknown is read
/// against nothing.
#[must_use]
pub fn standing(current: &[HistoryEntry], historical: &[HistoryEntry]) -> Vec<Finding> {
    let mut out = Vec::new();
    for cur in current.iter().filter(|c| known(&c.source)) {
        let own: Vec<HistoryEntry> = on::<System>(&on::<Variant>(historical, &cur.variant), &cur.system)
            .into_iter()
            .filter(|h| h.mode == cur.mode && h.n == cur.n && h.timestamp < cur.timestamp && known(&h.source))
            .collect();
        let Some(latest) = own.iter().max_by_key(|h| h.timestamp) else {
            continue;
        };
        let against = point_of(&own, &latest.source);
        let now = point_of(std::slice::from_ref(cur), &cur.source);
        let label = match (against.source == cur.source, step(&against, &now)) {
            (_, Move::Level) => continue,
            (true, _) => DRIFT,
            (false, Move::Slower) => REGRESSION,
            (false, Move::Faster) => FASTER,
        };
        let pct = if against.median_ns > 0.0 { (now.median_ns - against.median_ns) / against.median_ns * 100.0 } else { 0.0 };
        out.push(Finding {
            variant: cur.variant.clone(),
            label,
            pct,
            against: against.source,
        });
    }
    out
}

/// The label of `variant`'s finding, where it has one.
#[must_use]
pub fn label_for(findings: &[Finding], variant: &str) -> Option<&'static str> {
    findings
        .iter()
        .find(|f| f.variant == variant)
        .map(|f| f.label)
}

fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.2} us", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

/// The systems of one cell side by side: a row per variant, a column per
/// system, each cell the median of that system's latest run. `latest` holds
/// one entry per system with that run's rows.
#[must_use]
pub fn render_systems(latest: &[(String, Vec<HistoryEntry>)]) -> String {
    let systems: Vec<&str> = latest.iter().map(|(s, _)| s.as_str()).collect();
    let mut md = format!(
        "| variant | {} |\n|---|{}\n",
        systems.join(" | "),
        "---|".repeat(systems.len())
    );
    let all: Vec<HistoryEntry> = latest
        .iter()
        .flat_map(|(_, rows)| rows.iter().cloned())
        .collect();
    for (variant, _) in split::<Variant>(&all) {
        let cells: Vec<String> = latest
            .iter()
            .map(|(_, rows)| {
                on::<Variant>(rows, &variant)
                    .first()
                    .map_or_else(|| "-".to_string(), |e| fmt_ns(e.median_ns))
            })
            .collect();
        md.push_str(&format!("| {variant} | {} |\n", cells.join(" | ")));
    }
    md
}

/// The trail of every variant of one cell, system by system: each source point
/// in the order the runs visited it, how many runs stand at it, its median,
/// and how it stands against the point before it.
#[must_use]
pub fn render_trails(rows: &[HistoryEntry]) -> String {
    let mut md = String::from(
        "| system | variant | source | runs | median | against the point before |\n|---|---|---|---|---|---|\n",
    );
    for (system, at_system) in split::<System>(rows) {
        for (variant, own) in split::<Variant>(&at_system) {
            let points = trail(&own);
            for (i, p) in points.iter().enumerate() {
                let moved = match i.checked_sub(1).map(|j| step(&points[j], p)) {
                    None => "",
                    Some(Move::Slower) => REGRESSION,
                    Some(Move::Faster) => FASTER,
                    Some(Move::Level) => "level",
                };
                md.push_str(&format!(
                    "| {system} | {variant} | `{}` | {} | {} | {moved} |\n",
                    p.source,
                    p.runs,
                    fmt_ns(p.median_ns)
                ));
            }
            let unknown = own.iter().filter(|e| !known(&e.source)).count();
            if unknown > 0 {
                md.push_str(&format!("| {system} | {variant} | unknown | {unknown} | | not a point |\n"));
            }
        }
    }
    md
}

#[cfg(test)]
#[path = "axes_tests.rs"]
mod tests;
