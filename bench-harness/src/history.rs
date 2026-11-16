//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A cell's history: its runs' summary rows.
//!
//! A cell's history is its runs. Each run directory carries a
//! [`SUMMARY_FILE`](mockspace_bench_layout::SUMMARY_FILE) of per-variant
//! medians and confidence intervals, written once with the run, and
//! [`load_cell`] reads them back across every run of the cell on one machine
//! class. There is no ledger appended to in place: two machines' runs are two
//! sets of files that merge without touching each other. How the rows are
//! read against each other is [`crate::axes`].

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
    /// The code the row measured: a snapshot's lock id, a tracking arm's
    /// digest of what it built from, `-` where none was known, empty for a row
    /// written before the column existed. One axis of the row's point
    /// ([`crate::axes`]).
    pub source:     String,
    /// The system that ran it ([`crate::system`]). Empty only in a row
    /// written before the column existed, which [`load_cell`] fills from the
    /// run's machine class directory. One axis of the row's point.
    pub system:     String,
}

/// A run's summary file: the schema header, then one row per entry.
#[must_use]
pub fn render_summary(entries: &[HistoryEntry]) -> String {
    let mut out = format!("{SUMMARY_HEADER}\n");
    for e in entries {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\n",
            e.timestamp,
            e.git_commit,
            e.benchmark,
            e.variant,
            e.n,
            e.mode,
            e.median_ns,
            e.ci_lo_ns,
            e.ci_hi_ns,
            e.source,
            e.system
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
                source:     p.get(9).map(|s| s.trim().to_string()).unwrap_or_default(),
                system:     p.get(10).map(|s| s.trim().to_string()).unwrap_or_default(),
            });
        }
    }
    entries
}

/// The rows of one run's summary, each carrying its system: the row's own, or
/// the run's machine class where the row predates the column.
#[must_use]
pub fn run_rows(run: &mockspace_bench_layout::RunDir) -> Vec<HistoryEntry> {
    let Ok(text) = std::fs::read_to_string(run.dir.join(SUMMARY_FILE)) else {
        return Vec::new();
    };
    let mut rows = parse_summary(&text);
    for r in &mut rows {
        if r.system.is_empty() {
            r.system = run.machine.clone();
        }
    }
    rows
}

/// Every summary row of one cell on one machine class, across its runs, oldest
/// run first, each carrying its system ([`run_rows`]). Runs on other machine
/// classes are not read, and the rows are split further by their system, the
/// target included, wherever they are compared ([`crate::axes`]). A run with
/// no summary (one an `after_cell` hook failed) contributes nothing.
#[must_use]
pub fn load_cell(results_root: &Path, bench: &str, cell: &Cell, machine: &str) -> Vec<HistoryEntry> {
    mockspace_bench_layout::runs(results_root)
        .into_iter()
        .filter(|r| r.bench == bench && &r.cell == cell && r.machine == machine)
        .flat_map(|r| run_rows(&r))
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
            source:     "-".into(),
            system:     "linux-x86_64-c4-bare-amd".into(),
        }
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
        assert!(text.starts_with("# schema_v2\t"), "{text}");
        assert_eq!(parse_summary(&text), rows);
    }

    /// A row written before the `source` column existed, nine fields as the old
    /// ledger and the first summaries wrote it, still parses, and says it
    /// names no source rather than taking the next row's or a default one.
    #[test]
    fn a_row_from_before_the_source_column_reads_as_naming_no_source() {
        let old = "# schema_v1\tx\n7\tabc1234\thash_n64\tfnv1a\t64\twarm\t100.0\t98.0\t102.0\n";
        let rows = parse_summary(old);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "");
        assert_eq!(rows[0].median_ns, 100.0);
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

    /// Every loaded row names its system: a row from before the column takes
    /// the machine class of the run directory it sits in, and a row that
    /// names its own, a target included, keeps it.
    #[test]
    fn a_loaded_row_always_names_its_system() {
        let root = mockspace_manifest::scratch::held();
        let _ = std::fs::remove_dir_all(&root);
        let cell = Cell { sweep: "hash".into(), n: 64 };
        let here = "linux-x86_64-c4-bare-amd";
        let put = |run: &str, body: &str| {
            let dir = root.join(mockspace_bench_layout::run_rel("hash", &cell, here, run));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(SUMMARY_FILE), body).unwrap();
        };
        put("20261114T000000Z-a", "# schema_v1\n1\ta\thash_n64\tfnv1a\t64\twarm\t1.0\t1.0\t1.0\n");
        let mut own = entry("fnv1a", "warm", 2, 1.0, 1.0, 1.0);
        own.system = format!("{here}+target-cpu=native");
        put("20261115T000000Z-b", &render_summary(&[own]));
        let systems: Vec<String> = load_cell(&root, "hash", &cell, here).into_iter().map(|e| e.system).collect();
        assert_eq!(systems, [here.to_string(), format!("{here}+target-cpu=native")]);
        std::fs::remove_dir_all(&root).ok();
    }
}
