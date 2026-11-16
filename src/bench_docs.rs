//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Bench-results documentation generation.
//!
//! When the consumer's `mock/benches/bench.toml` declares
//! `[docgen] enabled = true`, the docs regeneration pass emits a
//! succinct, human-readable `BENCHES.md` under `docs/` from the bench
//! runs, plus one graphviz visualisation per cell and machine class (a
//! `.dot` rendered to PNG and embedded), so the latest measured numbers
//! live in the same generated documentation tree as everything else. The
//! doc points at `mock/benches/results/` for the full per-run data.
//!
//! The data source is the runs under `results/`: for each cell and
//! machine class, the latest run carrying a `summary.tsv` is the
//! "current" number per variant. Machine classes are kept apart, since a
//! number from one says nothing about another.

use std::fs;
use std::path::Path;
use std::process::Command;

use mockspace_bench_harness::axes;
use mockspace_bench_harness::history::{HistoryEntry, run_rows};
use mockspace_bench_harness::layout;

use crate::config::Config;
use crate::render_design;

/// Whether the consumer opted into bench docgen.
fn docgen_enabled(bench_dir: &Path) -> bool {
    let Ok(text) = fs::read_to_string(bench_dir.join("bench.toml")) else {
        return false;
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("docgen")
        .and_then(|d| d.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The latest run with a summary of every cell on every system:
/// `(<bench>/<cell>, system, run, its rows of that system)`, sorted by the
/// first two. A system is a variant of its own, so a machine class whose runs
/// were built for two targets gives two entries.
fn latest_runs(bench_dir: &Path) -> Vec<(String, String, String, Vec<HistoryEntry>)> {
    let mut latest: std::collections::BTreeMap<(String, String), (String, Vec<HistoryEntry>)> =
        std::collections::BTreeMap::new();
    // Oldest first within a machine, so the last one seen is the latest.
    for r in layout::runs(&bench_dir.join(layout::RESULTS_DIR)) {
        let rows = run_rows(&r);
        for (system, at) in axes::split::<axes::System>(&rows) {
            latest.insert((format!("{}/{}", r.bench, r.cell.name()), system), (r.run.clone(), at));
        }
    }
    latest
        .into_iter()
        .map(|((cell, system), (run, rows))| (cell, system, run, rows))
        .collect()
}

/// Every row of every run of `cell` (`<bench>/<cell>`), for its trails.
fn cell_rows(bench_dir: &Path, cell: &str) -> Vec<HistoryEntry> {
    layout::runs(&bench_dir.join(layout::RESULTS_DIR))
        .iter()
        .filter(|r| format!("{}/{}", r.bench, r.cell.name()) == cell)
        .flat_map(run_rows)
        .collect()
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

/// Build one benchmark's dot graph: variants ordered fastest to
/// slowest, node colour graded by relative speed, edges labelled with
/// the slowdown ratio to the previous rank.
fn benchmark_dot(benchmark: &str, rows: &[HistoryEntry]) -> String {
    let mut sorted: Vec<&HistoryEntry> = rows.iter().collect();
    sorted.sort_by(|a, b| a.median_ns.total_cmp(&b.median_ns));
    let best = sorted.first().map(|r| r.median_ns).unwrap_or(1.0).max(1e-9);
    let mut dot = String::new();
    dot.push_str(&format!(
        "digraph \"{benchmark}\" {{\n  rankdir=LR;\n  node [shape=box, style=\"rounded,filled\", fontname=\"Helvetica\"];\n"
    ));
    for (i, r) in sorted.iter().enumerate() {
        let ratio = r.median_ns / best;
        // Fastest is green, everything slower fades toward red as the
        // ratio grows; capped at 3x for the colour ramp.
        let t = ((ratio - 1.0) / 2.0).clamp(0.0, 1.0);
        let hue = 0.33 * (1.0 - t);
        dot.push_str(&format!(
            "  v{i} [label=\"{}\\n{} ({:.2}x)\", fillcolor=\"{:.3} 0.4 1.0\"];\n",
            r.variant,
            fmt_ns(r.median_ns),
            ratio,
            hue
        ));
    }
    for i in 1 .. sorted.len() {
        let step = sorted[i].median_ns / sorted[i - 1].median_ns;
        dot.push_str(&format!("  v{} -> v{i} [label=\"{step:.2}x\"];\n", i - 1));
    }
    dot.push_str("}\n");
    dot
}

/// Generate `docs/BENCHES.md` plus per-benchmark dot/png artifacts.
/// Called from the docs regeneration pass; a consumer without bench
/// docgen enabled (or without history) generates nothing.
/// Returns every path it wrote into `docs_dir`.
///
/// NOTE: the caller sweeps `docs_dir` of anything nothing generated, so a
/// generator that writes there and reports nothing has its output deleted on
/// the same run. That was true of this one and went unseen only because no
/// project in the workspace opts into `[docgen]` yet.
pub fn generate(cfg: &Config) -> Vec<std::path::PathBuf> {
    let mut wrote: Vec<std::path::PathBuf> = Vec::new();
    let bench_dir = cfg.mock_dir.join("benches");
    if !docgen_enabled(&bench_dir) {
        return wrote;
    }
    let runs = latest_runs(&bench_dir);
    if runs.is_empty() {
        eprintln!(
            "  bench docgen enabled but no run carries a summary under {}",
            bench_dir.join(layout::RESULTS_DIR).display()
        );
        return wrote;
    }

    eprintln!("--- generating BENCHES.md ---");
    let mut md = render_design::generation_header_md(cfg);
    md.push_str(
        "# Benchmarks\n\nLatest measured medians per cell and machine class, from \
         the latest run of each. Succinct by design: for the full data behind \
         any number (per-sample CSVs, environment metadata, statistical \
         findings), see `mock/benches/results/` in the repository.\n\n\
         Every system is a variant of its own and is shown beside the others, \
         never pooled with them. Runs at one source on one system are one \
         point: a snapshot has one source, its lock, so its number moves only \
         with the machine, the toolchain or the harness; a tracking arm's \
         source is the code it built from, and its trail of points is the \
         code's trend.\n\n",
    );

    let mut last_cell = String::new();
    for (cell, machine, run, rows) in &runs {
        if *cell != last_cell {
            last_cell = cell.clone();
            let latest: Vec<(String, Vec<HistoryEntry>)> =
                runs.iter().filter(|r| r.0 == *cell).map(|r| (r.1.clone(), r.3.clone())).collect();
            md.push_str(&format!(
                "## {cell}\n\n{}\n{}\n",
                axes::render_systems(&latest),
                axes::render_trails(&cell_rows(&bench_dir, cell))
            ));
        }
        let benchmark = format!("{cell} on {machine}");
        let file_key = format!("{}.{}", cell.replace('/', "."), machine.replace('+', "."));
        md.push_str(&format!("### {benchmark}\n\n"));
        md.push_str(&format!("Run `{run}`{}.\n\n", mockspace_bench_harness::kinds::measured(rows)));
        md.push_str("| variant | median | vs best |\n|---|---|---|\n");
        let best = rows
            .iter()
            .map(|r| r.median_ns)
            .fold(f64::INFINITY, f64::min)
            .max(1e-9);
        let mut sorted: Vec<&HistoryEntry> = rows.iter().collect();
        sorted.sort_by(|a, b| a.median_ns.total_cmp(&b.median_ns));
        for r in &sorted {
            md.push_str(&format!(
                "| {} | {} | {:.2}x |\n",
                r.variant,
                fmt_ns(r.median_ns),
                r.median_ns / best
            ));
        }
        md.push('\n');

        // Dot + rendered PNG, embedded when the render succeeds.
        let dot_name = render_design::ordered_doc_name(&format!("BENCHES.{file_key}.dot"), cfg);
        let png_name = render_design::ordered_doc_name(&format!("BENCHES.{file_key}.png"), cfg);
        let dot_path = cfg.docs_dir.join(&dot_name);
        let png_path = cfg.docs_dir.join(&png_name);
        let dot_content = format!(
            "{}{}",
            render_design::generation_header_dot(cfg),
            benchmark_dot(&benchmark, rows)
        );
        render_design::write_generated(&dot_path, &dot_content);
        wrote.push(dot_path.clone());
        wrote.push(png_path.clone());
        let rendered = Command::new("dot")
            .arg("-Tpng")
            .arg("-Gdpi=150")
            .arg(&dot_path)
            .arg("-o")
            .arg(&png_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if rendered {
            md.push_str(&format!("![{benchmark}]({png_name})\n\n"));
            eprintln!("  {}", png_path.display());
        } else {
            eprintln!("  dot render skipped for {benchmark} (is graphviz installed?)");
        }
    }

    let md_path = cfg
        .docs_dir
        .join(render_design::ordered_doc_name("BENCHES.md", cfg));
    render_design::write_generated(&md_path, &md);
    wrote.push(md_path.clone());
    eprintln!("  {}", md_path.display());
    wrote
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The latest run with a summary stands for its cell, per machine class:
    /// a newer run on one class does not hide another class's run, and a run
    /// with no summary is passed over rather than read as an empty table.
    #[test]
    fn each_cell_shows_its_latest_summarised_run_per_machine_class() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let results = dir.path().join(layout::RESULTS_DIR);
        let cell = layout::Cell { sweep: "width-l1".into(), n: 80003 };
        let put = |machine: &str, system: &str, run: &str, median: f64, summary: bool| {
            let d = results.join(layout::run_rel("warm", &cell, machine, run));
            fs::create_dir_all(&d).unwrap();
            if summary {
                let e = HistoryEntry {
                    timestamp:  1,
                    git_commit: "a".into(),
                    benchmark:  cell.name(),
                    variant:    "fnv".into(),
                    n:          80003,
                    mode:       "warm".into(),
                    median_ns:  median,
                    ci_lo_ns:   median,
                    ci_hi_ns:   median,
                    source:     "-".into(),
                    system:     system.into(),
                };
                fs::write(d.join(layout::SUMMARY_FILE), mockspace_bench_harness::render_summary(&[e])).unwrap();
            }
        };
        let linux = "linux-x86_64-c4-bare-amd";
        let native = "linux-x86_64-c4-bare-amd+target-cpu=native";
        let mac = "macos-aarch64-c8-bare-apple";
        put(linux, "", "20261114T000000Z-a", 10.0, true);
        put(linux, "", "20261115T000000Z-b", 11.0, true);
        put(linux, "", "20261116T000000Z-c", 12.0, false);
        // a later run on the same machine built for its own cpu: another system,
        // which neither hides nor is hidden by the plain build's latest run
        put(linux, native, "20261117T000000Z-d", 8.0, true);
        put(mac, "", "20261101T000000Z-z", 5.0, true);
        let got: Vec<(String, String, String, f64)> = latest_runs(dir.path())
            .into_iter()
            .map(|(c, m, r, rows)| (c, m, r, rows[0].median_ns))
            .collect();
        assert_eq!(got, vec![
            ("warm/width-l1_n80003".into(), linux.into(), "20261115T000000Z-b".into(), 11.0),
            ("warm/width-l1_n80003".into(), native.into(), "20261117T000000Z-d".into(), 8.0),
            ("warm/width-l1_n80003".into(), mac.into(), "20261101T000000Z-z".into(), 5.0),
        ]);
    }
}
