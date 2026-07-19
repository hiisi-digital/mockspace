//! Bench-results documentation generation.
//!
//! When the consumer's `mock/benches/bench.toml` declares
//! `[docgen] enabled = true`, the docs regeneration pass emits a
//! succinct, human-readable `BENCHES.md` under `docs/` from the bench
//! history, plus one graphviz visualisation per benchmark (a `.dot`
//! rendered to PNG and embedded), so the latest measured numbers live
//! in the same generated documentation tree as everything else. The
//! doc points at `mock/benches/results/` for the full per-run data
//! (CSV samples, meta, findings).
//!
//! The data source is the append-only bench history
//! (`mock/benches/.bench_history/<benchmark>.tsv`, written by the
//! bench driver on every run): the latest timestamp cohort per
//! benchmark is the "current" number per variant.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::Config;
use crate::render_design;

/// One parsed history row (subset of the harness's schema).
struct Row {
    timestamp:  u64,
    git_commit: String,
    variant:    String,
    median_ns:  f64,
}

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

/// Parse one `<benchmark>.tsv` history log and keep the latest
/// cohort (rows sharing the maximum timestamp) per variant.
fn latest_cohort(path: &Path) -> Vec<Row> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rows: Vec<Row> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 8 {
            continue;
        }
        let (Ok(ts), Ok(median)) = (f[0].parse::<u64>(), f[6].parse::<f64>()) else {
            continue;
        };
        rows.push(Row {
            timestamp:  ts,
            git_commit: f[1].to_string(),
            variant:    f[3].to_string(),
            median_ns:  median,
        });
    }
    let max_ts = rows.iter().map(|r| r.timestamp).max().unwrap_or(0);
    // The latest cohort: newest row per variant within a small window
    // of the maximum timestamp (one run appends all variants within
    // seconds of each other).
    let mut latest: BTreeMap<String, Row> = BTreeMap::new();
    for r in rows {
        if max_ts.saturating_sub(r.timestamp) <= 300 {
            latest.insert(r.variant.clone(), r);
        }
    }
    latest.into_values().collect()
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
fn benchmark_dot(benchmark: &str, rows: &[Row]) -> String {
    let mut sorted: Vec<&Row> = rows.iter().collect();
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
pub fn generate(cfg: &Config) {
    let bench_dir = cfg.mock_dir.join("benches");
    if !docgen_enabled(&bench_dir) {
        return;
    }
    let history_dir = bench_dir.join(".bench_history");
    let mut logs: Vec<std::path::PathBuf> = fs::read_dir(&history_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
                .collect()
        })
        .unwrap_or_default();
    logs.sort();
    if logs.is_empty() {
        eprintln!(
            "  bench docgen enabled but no history at {}",
            history_dir.display()
        );
        return;
    }

    eprintln!("--- generating BENCHES.md ---");
    let mut md = render_design::generation_header_md(cfg);
    md.push_str(
        "# Benchmarks\n\nLatest measured medians per benchmark, from the bench \
         framework's history log. Succinct by design: for the full data behind \
         any number (per-sample CSVs, environment metadata, statistical \
         findings), see `mock/benches/results/` in the repository.\n\n",
    );

    for log in &logs {
        let benchmark = log
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("bench")
            .to_string();
        let rows = latest_cohort(log);
        if rows.is_empty() {
            continue;
        }
        let commit = rows
            .iter()
            .max_by_key(|r| r.timestamp)
            .map(|r| r.git_commit.clone())
            .unwrap_or_default();
        md.push_str(&format!("## {benchmark}\n\n"));
        if !commit.is_empty() {
            md.push_str(&format!("Measured at commit `{commit}`.\n\n"));
        }
        md.push_str("| variant | median | vs best |\n|---|---|---|\n");
        let best = rows
            .iter()
            .map(|r| r.median_ns)
            .fold(f64::INFINITY, f64::min)
            .max(1e-9);
        let mut sorted: Vec<&Row> = rows.iter().collect();
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
        let dot_name = render_design::ordered_doc_name(&format!("BENCHES.{benchmark}.dot"), cfg);
        let png_name = render_design::ordered_doc_name(&format!("BENCHES.{benchmark}.png"), cfg);
        let dot_path = cfg.docs_dir.join(&dot_name);
        let png_path = cfg.docs_dir.join(&png_name);
        let dot_content = format!(
            "{}{}",
            render_design::generation_header_dot(cfg),
            benchmark_dot(&benchmark, &rows)
        );
        render_design::write_generated(&dot_path, &dot_content);
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
    eprintln!("  {}", md_path.display());
}
