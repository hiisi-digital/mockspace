//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Cross-benchmark meta-report.
//!
//! Reads multiple benchmark CSVs and correlates variant families
//! (e.g. asm, bitmask, fused, csp) across benchmarks. Identifies
//! which dispatch strategies consistently win or lose.

use std::collections::BTreeMap;

/// A summarised result for one variant in one benchmark.
pub struct VariantResult {
    pub variant:              String,
    pub benchmark:            String,
    pub n:                    usize,
    pub warm_median_ns:       f64,
    pub cold_median_ns:       f64,
    pub warm_pct_vs_baseline: f64,
    pub cold_pct_vs_baseline: f64,
}

/// Family classification based on variant name patterns.
///
/// Mockspace keeps the same heuristic family list as polka-dots so a
/// shared meta-report across consumers reads consistently. Add new
/// patterns here when a downstream consumer introduces a new
/// classification axis.
pub fn classify_family(variant: &str) -> &str {
    if variant.contains("asm") {
        return "asm";
    }
    if variant.contains("degmask") {
        return "degmask";
    }
    if variant.contains("bitmask") {
        return "bitmask";
    }
    if variant.contains("dsatur") {
        return "dsatur";
    }
    if variant.contains("fused") || variant.contains("match") {
        return "fused";
    }
    if variant.contains("csp") {
        return "csp";
    }
    if variant.contains("uninit") {
        return "uninit";
    }
    "other"
}

/// Generate a cross-benchmark meta-report from multiple CSV files.
///
/// `baseline_name` overrides the default baseline (the first variant
/// in each CSV). Pass `None` to keep the first-variant fallback.
pub fn generate(csv_paths: &[&str], baseline_name: Option<&str>) -> String {
    let mut all_results: Vec<VariantResult> = Vec::new();

    for path in csv_paths {
        if let Ok(text) = std::fs::read_to_string(path) {
            let parsed = parse_csv(&text, path, baseline_name);
            all_results.extend(parsed);
        }
    }

    if all_results.is_empty() {
        return "No data found in provided CSVs.\n".into();
    }

    let baseline_label = baseline_name.unwrap_or("first variant");

    let mut by_family: BTreeMap<&str, Vec<&VariantResult>> = BTreeMap::new();
    for r in &all_results {
        let family = classify_family(&r.variant);
        by_family.entry(family).or_default().push(r);
    }

    let mut md = String::new();
    md.push_str("# Cross-benchmark meta-report\n\n");
    md.push_str(&format!(
        "{} variants across {} benchmarks\n\n",
        all_results.len(),
        csv_paths.len()
    ));
    md.push_str(&format!("Baseline: **{}**\n\n", baseline_label));

    md.push_str("## Family summary (warm mode, % vs baseline)\n\n");
    md.push_str("| Family | count | mean Δ% | min Δ% | max Δ% | benchmarks |\n");
    md.push_str("|---|---|---|---|---|---|\n");

    for (family, results) in &by_family {
        let pcts: Vec<f64> = results.iter().map(|r| r.warm_pct_vs_baseline).collect();
        let n = pcts.len();
        if n == 0 {
            continue;
        }
        let mean = pcts.iter().sum::<f64>() / n as f64;
        let min = pcts.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = pcts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let benchmarks: Vec<&str> = results
            .iter()
            .map(|r| r.benchmark.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        md.push_str(&format!(
            "| {} | {} | {:+.1}% | {:+.1}% | {:+.1}% | {} |\n",
            family,
            n,
            mean,
            min,
            max,
            benchmarks.join(", ")
        ));
    }

    // ── Multi-N scaling ──
    let mut scaling: BTreeMap<(&str, &str), BTreeMap<usize, f64>> = BTreeMap::new();
    for r in &all_results {
        scaling
            .entry((&r.benchmark, &r.variant))
            .or_default()
            .insert(r.n, r.warm_median_ns);
    }

    let has_multi_n = scaling.values().any(|ns| ns.len() > 1);
    if has_multi_n {
        let all_ns: Vec<usize> = {
            let mut ns: Vec<usize> = scaling
                .values()
                .flat_map(|m| m.keys().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            ns.sort();
            ns
        };

        md.push_str("\n## Multi-N scaling (warm median ns)\n\n");
        md.push_str("| Benchmark | Variant |");
        for &n in &all_ns {
            md.push_str(&format!(" N={} |", n));
        }
        md.push_str(" Scale |\n|---|---|");
        for _ in &all_ns {
            md.push_str("---|");
        }
        md.push_str("---|\n");

        for ((bench, variant), ns_map) in &scaling {
            if ns_map.len() < 2 {
                continue;
            }
            md.push_str(&format!("| {} | {}", bench, variant));
            for &n in &all_ns {
                if let Some(&v) = ns_map.get(&n) {
                    md.push_str(&format!(" | {:.1}", v));
                } else {
                    md.push_str(" | -");
                }
            }
            // Scale factor: largest N / smallest N
            let min_n_val = all_ns.iter().filter_map(|n| ns_map.get(n)).next();
            let max_n_val = all_ns.iter().rev().filter_map(|n| ns_map.get(n)).next();
            match (min_n_val, max_n_val) {
                (Some(&lo), Some(&hi)) if lo > 0.0 => {
                    md.push_str(&format!(" | {:.1}x", hi / lo));
                },
                _ => md.push_str(" | -"),
            }
            md.push_str(" |\n");
        }
    }

    md
}

fn parse_csv(text: &str, path: &str, baseline_name: Option<&str>) -> Vec<VariantResult> {
    // `harness::write_csv` names its files `<sweep>_n<point>.csv`. Strip the
    // extension first: trimming a `_results.csv` suffix no writer in this crate
    // produces left `hash_n1024.csv` intact, so the size parsed out of
    // `1024.csv` and every row carried `n = 0`.
    let file = path.rsplit('/').next().unwrap_or(path);
    let bench_name = file.strip_suffix(".csv").unwrap_or(file);
    let bench_name = bench_name.strip_suffix("_results").unwrap_or(bench_name);
    let (benchmark, n) = match bench_name.rsplit_once("_n") {
        Some((stem, size)) => (stem, size.parse().unwrap_or(0)),
        None => (bench_name, 0usize),
    };

    let mut by_variant: BTreeMap<String, (Vec<f64>, Vec<f64>)> = BTreeMap::new();

    // Column order, from the one header both writers emit:
    // 0 run, 1 pass, 2 cooldown_ms, 3 mode, 4 variant, 5 batch_idx,
    // 6 e2e_ns, 7 algo_ns, 8 bridge_ns, ...
    // `warm_median_ns` and `cold_median_ns` are algorithm times, so this reads
    // column 7. Reading 6 reported the end-to-end time, bridge overhead and
    // workload stages included, under the algorithm's name.
    const MODE: usize = 3;
    const VARIANT: usize = 4;
    const ALGO_NS: usize = 7;

    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() <= ALGO_NS {
            continue;
        }
        let mode = cols[MODE];
        let variant = cols[VARIANT].to_string();
        let algo_ns: f64 = cols[ALGO_NS].parse().unwrap_or(0.0);
        let entry = by_variant
            .entry(variant)
            .or_insert_with(|| (Vec::new(), Vec::new()));
        if mode == "warm" {
            entry.0.push(algo_ns);
        } else {
            entry.1.push(algo_ns);
        }
    }

    // Use the named baseline if provided and present; else fall back
    // to the first variant in the CSV.
    let baseline_warm;
    let baseline_cold;
    if let Some(name) = baseline_name {
        if let Some((w, c)) = by_variant.get(name) {
            baseline_warm = median(w);
            baseline_cold = median(c);
        } else {
            // Named baseline not found in this CSV. Fall back to first.
            baseline_warm = by_variant
                .values()
                .next()
                .map(|(w, _)| median(w))
                .unwrap_or(1.0);
            baseline_cold = by_variant
                .values()
                .next()
                .map(|(_, c)| median(c))
                .unwrap_or(1.0);
        }
    } else {
        baseline_warm = by_variant
            .values()
            .next()
            .map(|(w, _)| median(w))
            .unwrap_or(1.0);
        baseline_cold = by_variant
            .values()
            .next()
            .map(|(_, c)| median(c))
            .unwrap_or(1.0);
    }

    by_variant
        .into_iter()
        .map(|(variant, (warm, cold))| {
            let wm = median(&warm);
            let cm = median(&cold);
            VariantResult {
                variant,
                benchmark: benchmark.to_string(),
                n,
                warm_median_ns: wm,
                cold_median_ns: cm,
                warm_pct_vs_baseline: if baseline_warm > 0.0 {
                    ((wm - baseline_warm) / baseline_warm) * 100.0
                } else {
                    0.0
                },
                cold_pct_vs_baseline: if baseline_cold > 0.0 {
                    ((cm - baseline_cold) / baseline_cold) * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect()
}

/// Delegates to [`crate::analysis::median`]: one definition of median in the
/// crate, so a cross-bench table and a per-bench report cannot print two
/// different numbers for the same samples.
fn median(vals: &[f64]) -> f64 {
    crate::analysis::median(vals)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CSV `harness::write_csv` writes, header and one row, in the exact
    /// column order the writer emits. Anything reading these files reads this
    /// shape or reads the wrong column.
    const CSV: &str = "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,\
batch_count,score,input_tag,instructions,cycles,setup_ns,first_ns,digest\n\
1,1,0,warm,alpha,0,900.0,100.0,800.0,5000,,,0,0,0.0,0.0,0\n\
1,1,0,warm,alpha,1,900.0,100.0,800.0,5000,,,0,0,0.0,0.0,0\n\
1,1,0,warm,beta,0,700.0,200.0,500.0,5000,,,0,0,0.0,0.0,0\n\
1,1,0,warm,beta,1,700.0,200.0,500.0,5000,,,0,0,0.0,0.0,0\n";

    /// `warm_median_ns` is documented and named as an algorithm time. Column 6
    /// is `e2e_ns`; `algo_ns` is column 7.
    #[test]
    fn the_warm_median_is_the_algo_column_not_the_end_to_end_one() {
        let rows = parse_csv(CSV, "results/hash/hash_n1024.csv", None);
        let alpha = rows.iter().find(|r| r.variant == "alpha").expect("alpha");
        assert_eq!(
            alpha.warm_median_ns, 100.0,
            "warm_median_ns read {} ns; algo_ns is 100 and e2e_ns is 900",
            alpha.warm_median_ns
        );
    }

    /// The harness writes `<sweep>_n<point>.csv`. `_results.csv` is a suffix no
    /// writer in this crate produces, so trimming it leaves the extension on
    /// and the size parses out of `64.csv`.
    #[test]
    fn the_size_is_recovered_from_the_filename_the_harness_writes() {
        let rows = parse_csv(CSV, "results/hash/hash_n1024.csv", None);
        assert!(!rows.is_empty(), "no rows parsed");
        for r in &rows {
            assert_eq!(r.n, 1024, "n parsed as {} from `hash_n1024.csv`", r.n);
            assert_eq!(r.benchmark, "hash", "benchmark parsed as `{}`", r.benchmark);
        }
    }

    /// A named baseline that is absent is a caller error, and falling back to
    /// "whichever variant sorts first" reports percentages against a baseline
    /// the caller did not ask for, with nothing in the output saying so.
    #[test]
    fn the_baseline_percentages_are_against_the_named_baseline() {
        let rows = parse_csv(CSV, "results/hash/hash_n1024.csv", Some("beta"));
        let alpha = rows.iter().find(|r| r.variant == "alpha").expect("alpha");
        // alpha 100ns against baseline beta 200ns: -50%.
        assert_eq!(
            alpha.warm_pct_vs_baseline, -50.0,
            "alpha vs named baseline beta read {}%",
            alpha.warm_pct_vs_baseline
        );
    }
}
