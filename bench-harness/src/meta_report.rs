//! Cross-benchmark meta-report.
//!
//! Reads multiple benchmark CSVs and correlates variant families
//! (e.g. asm, bitmask, fused, csp) across benchmarks. Identifies
//! which dispatch strategies consistently win or lose.

use std::collections::BTreeMap;

/// A summarised result for one variant in one benchmark.
pub struct VariantResult {
    pub variant: String,
    pub benchmark: String,
    pub n: usize,
    pub warm_median_ns: f64,
    pub cold_median_ns: f64,
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
            family, n, mean, min, max, benchmarks.join(", ")
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
                }
                _ => md.push_str(" | -"),
            }
            md.push_str(" |\n");
        }
    }

    md
}

fn parse_csv(text: &str, path: &str, baseline_name: Option<&str>) -> Vec<VariantResult> {
    let bench_name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches("_results.csv");
    let parts: Vec<&str> = bench_name.split("_n").collect();
    let benchmark = parts.first().copied().unwrap_or("unknown");
    let n: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut by_variant: BTreeMap<String, (Vec<f64>, Vec<f64>)> = BTreeMap::new();

    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 7 {
            continue;
        }
        let mode = cols[3];
        let variant = cols[4].to_string();
        let algo_ns: f64 = cols[6].parse().unwrap_or(0.0);
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

fn median(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}
