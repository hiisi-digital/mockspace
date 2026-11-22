//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Historical trend tracking and regression detection.
//!
//! A cell's history is its runs. Each run directory carries a
//! [`SUMMARY_FILE`](mockspace_bench_layout::SUMMARY_FILE) of per-variant
//! medians and confidence intervals, written once with the run, and
//! [`load_cell`] reads them back across every run of the cell on one machine
//! class. There is no ledger appended to in place: two machines' runs are two
//! sets of files that merge without touching each other, and a machine's
//! timings are never compared against another's.
//! [`detect_regressions`] flags entries whose current CI does not overlap the
//! historical baseline.

use std::path::Path;

use mockspace_bench_layout::{Cell, SUMMARY_FILE, SUMMARY_HEADER};

/// One historical data point.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub timestamp:  u64,
    pub git_commit: String,
    pub benchmark:  String,
    pub variant:    String,
    pub n:          usize,
    pub mode:       String,
    pub median_ns:  f64,
    pub ci_lo_ns:   f64,
    pub ci_hi_ns:   f64,
}

/// A run's summary file: the schema header, then one row per entry.
#[must_use]
pub fn render_summary(entries: &[HistoryEntry]) -> String {
    let mut out = format!("{SUMMARY_HEADER}\n");
    for e in entries {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\n",
            e.timestamp,
            e.git_commit,
            e.benchmark,
            e.variant,
            e.n,
            e.mode,
            e.median_ns,
            e.ci_lo_ns,
            e.ci_hi_ns
        ));
    }
    out
}

/// The rows of a summary file, or of an old ledger, which has the same schema.
/// Comment and header lines are skipped, and so is a row with too few fields.
#[must_use]
pub fn parse_summary(text: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() >= 9 {
            entries.push(HistoryEntry {
                timestamp:  p[0].parse().unwrap_or(0),
                git_commit: p[1].to_string(),
                benchmark:  p[2].to_string(),
                variant:    p[3].to_string(),
                n:          p[4].parse().unwrap_or(0),
                mode:       p[5].to_string(),
                median_ns:  p[6].parse().unwrap_or(0.0),
                ci_lo_ns:   p[7].parse().unwrap_or(0.0),
                ci_hi_ns:   p[8].parse().unwrap_or(0.0),
            });
        }
    }
    entries
}

/// Every summary row of one cell on one machine class, across its runs, oldest
/// run first. Runs on other machine classes are not read: a timing is compared
/// only with timings taken on the same class of machine. A run with no summary
/// (one an `after_cell` hook failed) contributes nothing.
#[must_use]
pub fn load_cell(results_root: &Path, bench: &str, cell: &Cell, machine: &str) -> Vec<HistoryEntry> {
    mockspace_bench_layout::runs(results_root)
        .into_iter()
        .filter(|r| r.bench == bench && &r.cell == cell && r.machine == machine)
        .filter_map(|r| std::fs::read_to_string(r.dir.join(SUMMARY_FILE)).ok())
        .flat_map(|text| parse_summary(&text))
        .collect()
}

/// The commit a summary row records: the run's marked commit, or `unknown`
/// where there was none, never an empty field.
#[must_use]
pub fn ledger_commit(commit: &str) -> String {
    match commit.trim() {
        "" => "unknown".to_string(),
        c => c.to_string(),
    }
}

/// One variant's standing against its own history, for one cell.
///
/// Named fields rather than a tuple: the previous shape was
/// `(String, String, f64, bool)` with the variant and the mode adjacent, and
/// its only caller read them as `(bench, variant, ...)`, so the operator's
/// regression line named the variant where the bench belonged and the mode
/// where the variant belonged, and the summary table matched the mode against
/// a variant name and therefore never flagged anything.
#[derive(Clone, Debug, PartialEq)]
pub struct Regression {
    /// The variant's exported bench name.
    pub variant:    String,
    /// `"warm"` or `"cold"`.
    pub mode:       String,
    /// Change against the historical median, already in percent. Positive is
    /// slower.
    pub pct_change: f64,
    /// The current confidence interval lies entirely above the historical one.
    pub flagged:    bool,
}

impl Regression {
    /// The operator's one-line report, with the bench this cell belongs to.
    #[must_use]
    pub fn render(&self, bench: &str) -> String {
        format!(
            "{bench} {} ({}) {:+.1}% vs history",
            self.variant, self.mode, self.pct_change
        )
    }
}

/// Whether `variant` regressed, over a set of [`Regression`] rows.
#[must_use]
pub fn flagged_for(rows: &[Regression], variant: &str) -> bool {
    rows.iter().any(|r| r.flagged && r.variant == variant)
}

/// Detect regressions against a rolling window of the last 5
/// historical entries.
pub fn detect_regressions(
    current: &[HistoryEntry],
    historical: &[HistoryEntry],
) -> Vec<Regression> {
    detect_regressions_window(current, historical, 5)
}

/// Detect regressions with an explicit rolling-window size.
///
/// For each `current` entry, the last `window_k` historical entries
/// for the same `(variant, mode, n)` are collected (older than
/// current's timestamp). The historical baseline is the median of
/// their medians; the historical CI upper bound is the median of
/// their CI uppers. A regression is flagged when current
/// `ci_lo > historical_ci_hi_median` (the new CI lies entirely above
/// the historical CI).
///
/// Returns one [`Regression`] per current entry that has history to compare
/// against. A variant with no history produces no row at all, which is how a
/// first run is told apart from a run that regressed nowhere.
pub fn detect_regressions_window(
    current: &[HistoryEntry],
    historical: &[HistoryEntry],
    window_k: usize,
) -> Vec<Regression> {
    let mut results = Vec::new();

    for curr in current {
        let mut prev_entries: Vec<&HistoryEntry> = historical
            .iter()
            .filter(|h| {
                h.variant == curr.variant
                    && h.mode == curr.mode
                    && h.n == curr.n
                    && h.timestamp < curr.timestamp
            })
            .collect();
        prev_entries.sort_by_key(|h| h.timestamp);
        let k = window_k.min(prev_entries.len());
        if k == 0 {
            continue;
        }
        let window = &prev_entries[prev_entries.len() - k ..];

        let mut hist_medians: Vec<f64> = window.iter().map(|h| h.median_ns).collect();
        hist_medians.sort_by(|a, b| a.total_cmp(b));
        let n = hist_medians.len();
        let hist_median = if n % 2 == 0 {
            (hist_medians[n / 2 - 1] + hist_medians[n / 2]) / 2.0
        } else {
            hist_medians[n / 2]
        };

        let mut hist_ci_hi: Vec<f64> = window.iter().map(|h| h.ci_hi_ns).collect();
        hist_ci_hi.sort_by(|a, b| a.total_cmp(b));
        let hist_ci_hi_median = if n % 2 == 0 {
            (hist_ci_hi[n / 2 - 1] + hist_ci_hi[n / 2]) / 2.0
        } else {
            hist_ci_hi[n / 2]
        };

        let pct = if hist_median > 0.0 {
            ((curr.median_ns - hist_median) / hist_median) * 100.0
        } else {
            0.0
        };

        let regressed = curr.ci_lo_ns > hist_ci_hi_median;

        results.push(Regression {
            variant:    curr.variant.clone(),
            mode:       curr.mode.clone(),
            pct_change: pct,
            flagged:    regressed,
        });
    }

    results
}

/// Current timestamp as Unix epoch seconds.
pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(variant: &str, mode: &str, ts: u64, med: f64, lo: f64, hi: f64) -> HistoryEntry {
        HistoryEntry {
            timestamp:  ts,
            git_commit: "abc1234".into(),
            benchmark:  "hash_n64".into(),
            variant:    variant.into(),
            n:          64,
            mode:       mode.into(),
            median_ns:  med,
            ci_lo_ns:   lo,
            ci_hi_ns:   hi,
        }
    }

    /// A regression row is read by name, not by position. The tuple this used
    /// to return was `(variant, mode, pct, regressed)` and its one caller
    /// destructured it as `(bench, variant, delta, flagged)`, so the operator's
    /// line printed the variant where the bench belonged and the mode where the
    /// variant belonged.
    #[test]
    fn a_regression_row_carries_the_variant_and_the_mode_in_their_own_fields() {
        let historical: Vec<HistoryEntry> = (1 ..= 5)
            .map(|i| entry("fnv1a", "warm", i, 100.0, 98.0, 102.0))
            .collect();
        let current = vec![entry("fnv1a", "warm", 99, 120.0, 118.0, 122.0)];
        let rows = detect_regressions(&current, &historical);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].variant, "fnv1a");
        assert_eq!(rows[0].mode, "warm");
        assert!(
            rows[0].flagged,
            "ci_lo 118 sits above the historical ci_hi 102"
        );
    }

    /// `pct_change` is already a percentage. The caller multiplied it by 100
    /// again, so a 20% regression was printed as `+2000.0%`.
    #[test]
    fn the_rendered_line_states_the_percentage_once() {
        let historical: Vec<HistoryEntry> = (1 ..= 5)
            .map(|i| entry("fnv1a", "warm", i, 100.0, 98.0, 102.0))
            .collect();
        let current = vec![entry("fnv1a", "warm", 99, 120.0, 118.0, 122.0)];
        let rows = detect_regressions(&current, &historical);
        assert!(
            (rows[0].pct_change - 20.0).abs() < 1e-9,
            "pct_change is {} for a 100ns -> 120ns move",
            rows[0].pct_change
        );
        let line = rows[0].render("hash");
        assert!(
            line.contains("+20.0%"),
            "the operator's line reads `{line}`"
        );
        assert!(line.contains("hash"), "the bench is named: `{line}`");
        assert!(line.contains("fnv1a"), "the variant is named: `{line}`");
    }

    /// The summary table's regression column is matched against the variant.
    /// Matching the mode field against a variant name can never be true, so the
    /// column was structurally always false and no run has ever marked one.
    #[test]
    fn the_summary_column_flags_the_variant_that_regressed() {
        let historical: Vec<HistoryEntry> = (1 ..= 5)
            .flat_map(|i| {
                [
                    entry("fnv1a", "warm", i, 100.0, 98.0, 102.0),
                    entry("xxhash", "warm", i, 100.0, 98.0, 102.0),
                ]
            })
            .collect();
        let current = vec![
            entry("fnv1a", "warm", 99, 120.0, 118.0, 122.0),
            entry("xxhash", "warm", 99, 100.0, 98.0, 102.0),
        ];
        let rows = detect_regressions(&current, &historical);
        assert!(
            flagged_for(&rows, "fnv1a"),
            "fnv1a regressed and is not flagged"
        );
        assert!(!flagged_for(&rows, "xxhash"), "xxhash did not regress");
        assert!(
            !flagged_for(&rows, "warm"),
            "`warm` is a mode, not a variant"
        );
    }

    /// A first run has no history to compare against, so it reports nothing
    /// rather than reporting no regression, and the caller can tell the two
    /// apart.
    #[test]
    fn a_variant_with_no_history_produces_no_row() {
        let current = vec![entry("fnv1a", "warm", 99, 120.0, 118.0, 122.0)];
        assert!(detect_regressions(&current, &[]).is_empty());
        // History under a different mode or size is not this cell's history.
        let other = vec![entry("fnv1a", "cold", 1, 100.0, 98.0, 102.0)];
        assert!(detect_regressions(&current, &other).is_empty());
    }

    /// The run's commit is the one its rows record, and a run with none (taken
    /// outside a git tree) records `unknown` rather than an empty field, which a
    /// reader would take as the commit a measurement was taken at.
    #[test]
    fn the_commit_is_never_recorded_as_an_empty_string() {
        assert_eq!(ledger_commit(""), "unknown");
        assert_eq!(ledger_commit("  "), "unknown");
        assert_eq!(ledger_commit("abc1234-dirty"), "abc1234-dirty");
    }

    #[test]
    fn a_summary_round_trips_and_skips_its_header() {
        let rows = vec![entry("fnv1a", "warm", 7, 100.0, 98.0, 102.0)];
        let text = render_summary(&rows);
        assert!(text.starts_with("# schema_v1\t"), "{text}");
        assert_eq!(parse_summary(&text), rows);
    }

    /// History is read across runs of one machine class only: a run on another
    /// class, or of another cell, never becomes this cell's baseline.
    #[test]
    fn a_cell_s_history_is_its_runs_on_one_machine_class() {
        let root = mockspace_manifest::scratch::held();
        let _ = std::fs::remove_dir_all(&root);
        let cell = Cell { sweep: "hash".into(), n: 64 };
        let other_cell = Cell { sweep: "hash".into(), n: 128 };
        let here = "linux-x86_64-c4-bare-amd";
        let there = "macos-aarch64-c8-bare-apple";
        let put = |cell: &Cell, machine: &str, run: &str, ts: u64| {
            let dir = root.join(mockspace_bench_layout::run_rel("hash", cell, machine, run));
            std::fs::create_dir_all(&dir).unwrap();
            let rows = [entry("fnv1a", "warm", ts, 100.0, 98.0, 102.0)];
            std::fs::write(dir.join(SUMMARY_FILE), render_summary(&rows)).unwrap();
        };
        put(&cell, here, "20261114T000000Z-a", 1);
        put(&cell, here, "20261115T000000Z-b", 2);
        put(&cell, there, "20261115T000000Z-b", 3);
        put(&other_cell, here, "20261115T000000Z-b", 4);
        let stamps: Vec<u64> = load_cell(&root, "hash", &cell, here).iter().map(|e| e.timestamp).collect();
        assert_eq!(stamps, [1, 2]);
        std::fs::remove_dir_all(&root).ok();
    }
}
