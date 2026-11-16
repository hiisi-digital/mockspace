//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Summary rows, human units, and the index over every run.

use std::collections::BTreeMap;
use std::path::Path;

use mockspace_bench_layout as layout;

use crate::error::BenchError;

/// One row of the end-of-run summary.
pub(super) struct SummaryRow {
    pub(super) bench:         String,
    pub(super) n:             usize,
    pub(super) variant:       String,
    pub(super) median_ns:     f64,
    pub(super) ratio_vs_best: f64,
    /// What the row's move against history is called by its bench's kind,
    /// where it moved: `REGRESSION` or `DRIFT`.
    pub(super) moved:         Option<&'static str>,
}

pub(super) fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.2} us", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

/// Regenerate `results/INDEX.md` from the runs under `root`.
///
/// Derived from the run directories alone, so two machines that each ran a
/// bench produce the same index once both runs are in the tree, and a
/// report-only pass or a run that measured nothing leaves every earlier row
/// standing because the rows are read back from the runs that hold them. Per
/// cell and machine class: the latest run with a summary, its rows, how many
/// runs there are, and a link to every run's report.
pub(super) fn write_index(root: &Path) -> Result<(), BenchError> {
    if !root.exists() {
        return Ok(());
    }
    std::fs::write(root.join(layout::INDEX_FILE), render_index(root))
        .map_err(|e| BenchError::io("writing results/INDEX.md", e))
}

fn render_index(root: &Path) -> String {
    let mut md = String::from(
        "# Bench Results Index\n\nGenerated from the runs under `results/`, one directory \
         per run at `<bench>/<cell>/<machine class>/<run>/`, and regenerated from them on \
         every run. Each run holds its samples, meta, report and summary.\n",
    );
    let runs = layout::runs(root);
    let mut groups: BTreeMap<(String, String, String), Vec<&layout::RunDir>> = BTreeMap::new();
    for r in &runs {
        groups
            .entry((r.bench.clone(), r.cell.name(), r.machine.clone()))
            .or_default()
            .push(r);
    }
    let mut last_cell = (String::new(), String::new());
    for ((bench, cell, machine), group) in &groups {
        if (bench, cell) != (&last_cell.0, &last_cell.1) {
            md.push_str(&format!("\n## {bench}/{cell}\n"));
            last_cell = (bench.clone(), cell.clone());
            md.push_str(&cell_across_systems(&runs, bench, cell));
        }
        md.push_str(&format!("\n### {machine}, {} run(s)\n\n", group.len()));
        let latest = group
            .iter()
            .rev()
            .map(|r| (r, crate::history::run_rows(r)))
            .find(|(_, rows)| !rows.is_empty());
        if let Some((r, rows)) = latest {
            let best = rows.iter().map(|e| e.median_ns).fold(f64::INFINITY, f64::min);
            md.push_str(&format!(
                "Latest: `{}`{}\n\n| variant | median | vs best |\n|---|---|---|\n",
                r.run,
                measured(&rows)
            ));
            for e in &rows {
                let ratio = if best > 0.0 { e.median_ns / best } else { 1.0 };
                md.push_str(&format!("| {} | {} | {ratio:.2}x |\n", e.variant, fmt_ns(e.median_ns)));
            }
            md.push('\n');
        }
        for r in group {
            if r.dir.join(layout::REPORT_FILE).is_file() {
                let rel = r
                    .dir
                    .join(layout::REPORT_FILE)
                    .strip_prefix(root)
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                md.push_str(&format!("- [{}]({rel})\n", r.run));
            }
        }
    }
    md
}

/// One cell read across every system that ran it: the systems side by side,
/// each by its latest run, and every system's trail of source points. Empty
/// where no run of the cell has a summary.
fn cell_across_systems(runs: &[layout::RunDir], bench: &str, cell: &str) -> String {
    let mut rows: Vec<crate::history::HistoryEntry> = Vec::new();
    // Per system, the rows of its latest run; the runs come oldest first
    // within a machine class, so a later run of a system replaces its entry.
    let mut latest: Vec<(String, Vec<crate::history::HistoryEntry>)> = Vec::new();
    for r in runs.iter().filter(|r| r.bench == bench && r.cell.name() == cell) {
        let at_run = crate::history::run_rows(r);
        for (system, at) in axes::split::<axes::System>(&at_run) {
            match latest.iter_mut().find(|(s, _)| *s == system) {
                Some(slot) => slot.1 = at,
                None => latest.push((system, at)),
            }
        }
        rows.extend(at_run);
    }
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "\nEach system is a variant of its own, its latest run side by side with the others':\n\n{}\n\
         Each system's source points, in the order its runs visited them:\n\n{}",
        axes::render_systems(&latest),
        axes::render_trails(&rows)
    )
}

use crate::axes;
use crate::kinds::measured;

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = "linux-x86_64-c4-bare-amd";

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root =
            mockspace_manifest::scratch::held();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn run(root: &Path, machine: &str, run: &str, median: f64, summary: bool) {
        let cell = layout::Cell { sweep: "hash".into(), n: 64 };
        let dir = root.join(layout::run_rel("hash", &cell, machine, run));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(layout::REPORT_FILE), "# f\n").unwrap();
        if summary {
            let e = crate::history::HistoryEntry {
                timestamp:  1,
                git_commit: "a".into(),
                benchmark:  "hash_n64".into(),
                variant:    "fnv1a".into(),
                n:          64,
                mode:       "warm".into(),
                median_ns:  median,
                ci_lo_ns:   median,
                ci_hi_ns:   median,
                source:     "-".into(),
                system:     String::new(),
            };
            std::fs::write(dir.join(layout::SUMMARY_FILE), crate::history::render_summary(&[e])).unwrap();
        }
    }

    #[test]
    fn the_index_shows_each_machine_s_latest_run_and_links_every_report() {
        let root = temp_root("write");
        run(&root, M, "20261114T000000Z-a", 10.0, true);
        run(&root, M, "20261115T000000Z-b", 12.0, true);
        run(&root, "macos-aarch64-c8-bare-apple", "20261115T000000Z-b", 7.0, true);
        write_index(&root).unwrap();
        let md = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(md.contains("## hash/hash_n64"), "{md}");
        assert!(md.contains(&format!("### {M}, 2 run(s)")), "{md}");
        assert!(md.contains(&format!("Latest: `20261115T000000Z-b`, on `{M}` at `a`")), "{md}");
        assert!(md.contains("| fnv1a | 12 ns |"), "the latest run's row, not the older one:\n{md}");
        assert!(md.contains("| fnv1a | 7 ns |"), "the other machine's run:\n{md}");
        // the two systems side by side, each by its latest run, never pooled
        assert!(
            md.contains(&format!("| variant | {M} | macos-aarch64-c8-bare-apple |\n|---|---|---|\n| fnv1a | 12 ns | 7 ns |")),
            "{md}"
        );
        // each system's points: both of M's runs stand at one source, so one point of two runs
        assert!(md.contains(&format!("| {M} | fnv1a | `-` | 2 | 11 ns |  |")), "{md}");
        assert!(md.contains(&format!("hash/hash_n64/{M}/20261114T000000Z-a/report.md")), "{md}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// `--report-only` measures nothing and a run whose cells all failed leaves
    /// no summary, and the index rebuilt after either still carries the last
    /// measured run's rows, read back from that run's own summary.
    #[test]
    fn a_run_with_no_summary_leaves_the_last_measured_rows_standing() {
        let root = temp_root("preserve");
        run(&root, M, "20261114T000000Z-a", 10.0, true);
        run(&root, M, "20261115T000000Z-b", 99.0, false);
        write_index(&root).unwrap();
        let md = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(md.contains("Latest: `20261114T000000Z-a`"), "{md}");
        assert!(md.contains("| fnv1a | 10 ns |"), "{md}");
        assert!(!md.contains("99 ns"), "{md}");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A fresh tree with nothing measured yet has no rows to show and must not
    /// invent one.
    #[test]
    fn a_tree_with_no_runs_writes_no_table() {
        let root = temp_root("fresh");
        write_index(&root).unwrap();
        let md = std::fs::read_to_string(root.join("INDEX.md")).unwrap();
        assert!(!md.contains("Latest:"), "{md}");
        assert!(!md.contains("## "), "{md}");
        std::fs::remove_dir_all(&root).ok();
    }
}
