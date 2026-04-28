//! Markdown report generation from analysis results.
//!
//! Builds a `findings.md` from a [`crate::analysis::DataSet`]:
//! header + key findings, methodology, end-to-end + algo-only +
//! per-cooldown tables, optional throughput / performance-model
//! tables when `ops_per_call > 0`, statistical comparison with
//! Benjamini-Hochberg FDR-adjusted p-values, per-pass consistency,
//! per-cooldown coefficient of variation, ASCII histogram of the
//! algo distribution per variant, and an automated diagnostic
//! section.

use crate::analysis::{bh_fdr_adjust, bootstrap_ci_median, compare, pct_delta, DataSet};

/// Generate a complete markdown report from a [`DataSet`].
pub fn generate(ds: &DataSet, title: &str) -> String {
    let mut md = String::new();
    let base = ds.baseline();

    // ── Header ──
    md.push_str(&format!("# {}\n\n", title));
    md.push_str(&format!(
        "{} variants, {} samples per variant.\n",
        ds.variants.len(),
        base.e2e_all.count
    ));
    md.push_str(&format!("Baseline: **{}**\n\n", base.name));

    if !ds.drift_note.is_empty() {
        md.push_str(&format!("> **WARNING:** {}\n\n", ds.drift_note));
    }

    // ── Executive summary ──
    if ds.variants.len() > 1 {
        md.push_str("## Key findings\n\n");

        let mut by_median: Vec<(usize, f64)> = ds
            .variants
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.algo_all.median))
            .collect();
        by_median.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let (fastest_idx, fastest_med) = by_median[0];
        let fastest = &ds.variants[fastest_idx];
        let base_med = base.algo_all.median;
        let fastest_pct = if base_med > 0.0 {
            (fastest_med - base_med) / base_med * 100.0
        } else {
            0.0
        };

        if fastest_idx == ds.baseline_idx {
            md.push_str(&format!(
                "- **Baseline ({}) is the fastest** at {:.1} ns median\n",
                fastest.name, fastest_med
            ));
        } else {
            md.push_str(&format!(
                "- **Fastest: {}** at {:.1} ns median ({:+.1}% vs baseline)\n",
                fastest.name, fastest_med, fastest_pct
            ));
        }

        let base_vals: Vec<f64> = base.keyed_algo.iter().map(|t| t.3).collect();
        let mut improvements = 0usize;
        let mut regressions = 0usize;
        for (i, v) in ds.variants.iter().enumerate() {
            if i == ds.baseline_idx {
                continue;
            }
            let v_vals: Vec<f64> = v.keyed_algo.iter().map(|t| t.3).collect();
            let cmp = compare(&v_vals, &base_vals, 0xCAFE);
            if cmp.significant {
                if cmp.median_diff_ns < 0.0 {
                    improvements += 1;
                } else {
                    regressions += 1;
                }
            }
        }
        if improvements > 0 {
            md.push_str(&format!(
                "- {} variant{} significantly faster than baseline\n",
                improvements,
                if improvements == 1 { "" } else { "s" }
            ));
        }
        if regressions > 0 {
            md.push_str(&format!(
                "- {} variant{} significantly slower than baseline\n",
                regressions,
                if regressions == 1 { "" } else { "s" }
            ));
        }

        let (_, slowest_med) = by_median[by_median.len() - 1];
        if fastest_med > 0.0 {
            md.push_str(&format!(
                "- Spread: {:.2}x (fastest {:.1} ns, slowest {:.1} ns)\n",
                slowest_med / fastest_med, fastest_med, slowest_med
            ));
        }

        md.push('\n');
    }

    // ── Methodology ──
    let m = &ds.meta;
    if m.passes > 0 || m.harness_runs > 0 {
        md.push_str("## Methodology\n\n");
        md.push_str("| Parameter | Value |\n|---|---|\n");
        md.push_str(&format!("| Workload | {} |\n", title));
        if m.passes > 0 {
            md.push_str(&format!("| Passes | {} |\n", m.passes));
        }
        if m.runs_per_pass > 0 {
            md.push_str(&format!("| Runs per pass | {} |\n", m.runs_per_pass));
        }
        if m.batch_size > 0 {
            md.push_str(&format!("| Batch size | {} |\n", m.batch_size));
        }
        if m.harness_runs > 0 {
            md.push_str(&format!("| Harness runs | {} |\n", m.harness_runs));
        }
        if !m.cooldowns_ms.is_empty() {
            let cds: Vec<String> = m.cooldowns_ms.iter().map(|c| format!("{}ms", c)).collect();
            md.push_str(&format!("| Cooldown schedule | {} |\n", cds.join(", ")));
        }
        if m.master_seed != 0 {
            md.push_str(&format!("| Master seed | {:#018x} |\n", m.master_seed));
        }
        if m.counter_freq > 0 {
            md.push_str(&format!("| Counter frequency | {} Hz |\n", m.counter_freq));
        }
        md.push_str(&format!("| Drift correction | {} |\n", m.drift_correction));
        md.push('\n');
    }

    // ── End-to-end combined ──
    md.push_str("## End-to-end (all cooldowns combined)\n\n");
    md.push_str("| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |\n");
    md.push_str("|---|---|---|---|---|---|---|\n");
    for v in &ds.variants {
        let d = pct_delta(v.e2e_all.mean, base.e2e_all.mean);
        let label = if v.name == base.name {
            "base".into()
        } else {
            format!("{:+.2}%", d)
        };
        md.push_str(&format!(
            "| {} | {:.0}ns | {:.0}ns | {:.0}ns | {:.0}ns | {:.0}ns | {} |\n",
            v.name, v.e2e_all.mean, v.e2e_all.median,
            v.e2e_all.best_20pct, v.e2e_all.mid_60pct, v.e2e_all.worst_20pct,
            label
        ));
    }

    // ── Function-under-test only, optionally with throughput ──
    let ops = ds.meta.ops_per_call;
    if ops > 0 {
        md.push_str("\n## Function-under-test only (all cooldowns combined)\n\n");
        md.push_str("| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |\n");
        md.push_str("|---|---|---|---|---|---|\n");
        for v in &ds.variants {
            let d = pct_delta(v.algo_all.mean, base.algo_all.mean);
            let label = if v.name == base.name {
                "base".into()
            } else {
                format!("{:+.2}%", d)
            };
            // throughput in Gops/s = ops / algo_ns
            let throughput_gops = if v.algo_all.mean > 0.0 {
                ops as f64 / v.algo_all.mean
            } else {
                0.0
            };
            md.push_str(&format!(
                "| {} | {:.0}ns | {:.0}ns | {:.0}ns | {} | {:.3} |\n",
                v.name, v.algo_all.mean,
                v.algo_all.best_20pct, v.algo_all.worst_20pct,
                label, throughput_gops
            ));
        }
    } else {
        md.push_str("\n## Function-under-test only (all cooldowns combined)\n\n");
        md.push_str("| Variant | mean | best 20% | worst 20% | Δ mean |\n");
        md.push_str("|---|---|---|---|---|\n");
        for v in &ds.variants {
            let d = pct_delta(v.algo_all.mean, base.algo_all.mean);
            let label = if v.name == base.name {
                "base".into()
            } else {
                format!("{:+.2}%", d)
            };
            md.push_str(&format!(
                "| {} | {:.0}ns | {:.0}ns | {:.0}ns | {} |\n",
                v.name, v.algo_all.mean,
                v.algo_all.best_20pct, v.algo_all.worst_20pct,
                label
            ));
        }
    }

    // ── Performance model (roofline) ──
    if ops > 0 {
        md.push_str("\n## Performance model\n\n");

        let mut peak_gops = 0.0f64;
        let mut peak_variant = "";
        for v in &ds.variants {
            if v.algo_all.best_20pct > 0.0 {
                let gops = ops as f64 / v.algo_all.best_20pct;
                if gops > peak_gops {
                    peak_gops = gops;
                    peak_variant = &v.name;
                }
            }
        }
        md.push_str(&format!(
            "- Peak throughput: **{:.3} Gops/s** ({}; best 20% batches)\n",
            peak_gops, peak_variant
        ));
        md.push_str(&format!("- Ops per call: {}\n", ops));

        md.push_str("\n| Variant | Gops/s (median) | % of peak |\n|---|---|---|\n");
        for v in &ds.variants {
            if v.algo_all.median > 0.0 {
                let gops = ops as f64 / v.algo_all.median;
                let pct = if peak_gops > 0.0 {
                    gops / peak_gops * 100.0
                } else {
                    0.0
                };
                md.push_str(&format!("| {} | {:.3} | {:.1}% |\n", v.name, gops, pct));
            }
        }
    }

    // ── Per-cooldown breakdown ──
    md.push_str("\n## Per-cooldown breakdown (e2e mean)\n\n");
    let cds: Vec<u64> = base.e2e_per_cd.keys().cloned().collect();
    md.push_str("| Variant |");
    for cd in &cds {
        md.push_str(&format!(" {}ms |", cd));
    }
    md.push_str(" avg | Δ avg |\n|---|");
    for _ in &cds {
        md.push_str("---|");
    }
    md.push_str("---|---|\n");

    for v in &ds.variants {
        md.push_str(&format!("| {}", v.name));
        let mut sum = 0.0;
        for cd in &cds {
            if let Some(s) = v.e2e_per_cd.get(cd) {
                md.push_str(&format!(" | {:.0}ns", s.mean));
                sum += s.mean;
            } else {
                md.push_str(" | - ");
            }
        }
        let avg = sum / cds.len().max(1) as f64;
        let base_avg: f64 = cds
            .iter()
            .filter_map(|cd| base.e2e_per_cd.get(cd).map(|s| s.mean))
            .sum::<f64>()
            / cds.len().max(1) as f64;
        let d = pct_delta(avg, base_avg);
        let label = if v.name == base.name {
            "base".into()
        } else {
            format!("{:+.2}%", d)
        };
        md.push_str(&format!(" | {:.0}ns | {} |\n", avg, label));
    }

    // ── Statistical comparison with BH-FDR correction ──
    md.push_str("\n## Statistical comparison (algo, 95% bootstrap CI)\n\n");
    md.push_str("| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|\n");

    let base_keyed = &ds.variants[ds.baseline_idx].keyed_algo;
    let base_algo: Vec<f64> = base_keyed.iter().map(|&(_, _, _, v)| v).collect();
    let (base_ci_lo, base_med, base_ci_hi) = bootstrap_ci_median(&base_algo, 0xB007_5747);

    struct CmpRow {
        variant_idx: usize,
        v_med: f64,
        v_ci_lo: f64,
        v_ci_hi: f64,
        diff_label: String,
        ci_delta_label: String,
        sig_label: &'static str,
        sign_p: f64,
        ties_label: String,
    }

    let mut rows: Vec<CmpRow> = Vec::new();
    let mut p_values: Vec<(usize, f64)> = Vec::new();

    for (i, v) in ds.variants.iter().enumerate() {
        if i == ds.baseline_idx {
            continue;
        }

        // Re-pair variant samples against base by matching (run, pass, cooldown_ms).
        let mut variant_paired: Vec<f64> = Vec::new();
        let mut base_paired: Vec<f64> = Vec::new();
        {
            let mut vi = 0;
            let mut bi = 0;
            let v_keyed = &v.keyed_algo;
            while vi < v_keyed.len() && bi < base_keyed.len() {
                let (vrun, vpass, vcd, vval) = v_keyed[vi];
                let (brun, bpass, bcd, bval) = base_keyed[bi];
                match (vrun, vpass, vcd).cmp(&(brun, bpass, bcd)) {
                    std::cmp::Ordering::Equal => {
                        variant_paired.push(vval);
                        base_paired.push(bval);
                        vi += 1;
                        bi += 1;
                    }
                    std::cmp::Ordering::Less => {
                        vi += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        bi += 1;
                    }
                }
            }
        }

        // Fall back to positional pairing if no key matches
        // (e.g. single-run data).
        let (variant_algo, base_algo_paired) = if variant_paired.is_empty() {
            let va: Vec<f64> = v.keyed_algo.iter().map(|&(_, _, _, x)| x).collect();
            (va, base_algo.clone())
        } else {
            (variant_paired, base_paired)
        };

        let (v_ci_lo, v_med, v_ci_hi) =
            bootstrap_ci_median(&variant_algo, 0xB007_5747 ^ i as u64);
        let cmp = compare(&variant_algo, &base_algo_paired, 0xC0C0_CAFE ^ i as u64);

        let ci_delta_label = format!("[{:+.0}, {:+.0}]ns", cmp.ci_lo_ns, cmp.ci_hi_ns);

        let sig_label = if cmp.significant { "YES" } else { "no" };
        let diff_label = if cmp.significant {
            format!("{:+.1}ns ({:+.1}%)", cmp.median_diff_ns, cmp.pct)
        } else {
            "no significant difference".into()
        };

        // Warn when ties exceed 10% of total paired samples. The sign
        // test drops ties, so a high tie fraction reduces effective
        // sample size.
        let total_pairs = variant_algo.len().min(base_algo_paired.len()) as u32;
        let ties_label = if total_pairs > 0 && cmp.ties * 10 > total_pairs {
            format!(
                "**{}** ({:.0}%, HIGH)",
                cmp.ties,
                cmp.ties as f64 / total_pairs as f64 * 100.0
            )
        } else if cmp.ties > 0 {
            format!("{}", cmp.ties)
        } else {
            "0".into()
        };

        p_values.push((rows.len(), cmp.sign_test_p));

        rows.push(CmpRow {
            variant_idx: i,
            v_med,
            v_ci_lo,
            v_ci_hi,
            diff_label,
            ci_delta_label,
            sig_label,
            sign_p: cmp.sign_test_p,
            ties_label,
        });
    }

    // Apply Benjamini-Hochberg FDR correction
    bh_fdr_adjust(&mut p_values);
    let mut adj_p_by_row = vec![1.0f64; rows.len()];
    for (row_idx, adj_p) in &p_values {
        adj_p_by_row[*row_idx] = *adj_p;
    }

    // Baseline row
    md.push_str(&format!(
        "| {} | {:.0}ns | base | --- | [{:.0}, {:.0}] | --- | --- | --- | --- |\n",
        base.name, base_med, base_ci_lo, base_ci_hi
    ));

    for (row_idx, row) in rows.iter().enumerate() {
        let adj_p = adj_p_by_row[row_idx];
        let adj_sig = if adj_p < 0.05 { "YES" } else { "no" };
        let sig_display = if adj_sig != row.sig_label {
            format!("{} (adj: {})", row.sig_label, adj_sig)
        } else {
            row.sig_label.to_string()
        };
        let v = &ds.variants[row.variant_idx];
        md.push_str(&format!(
            "| {} | {:.0}ns | {} | {} | [{:.0}, {:.0}] | {} | {:.4} | {:.4} | {} |\n",
            v.name, row.v_med, row.diff_label, row.ci_delta_label,
            row.v_ci_lo, row.v_ci_hi, sig_display,
            adj_p, row.sign_p, row.ties_label
        ));
    }

    // ── Per-pass consistency (nonstop) with autocorrelation ──
    md.push_str("\n## Per-pass consistency (nonstop e2e, Δ vs baseline)\n\n");
    let n_passes = base.nonstop_per_pass.len();
    if n_passes > 0 {
        md.push_str("| Pass |");
        md.push_str(&format!(" {} |", base.name));
        for v in &ds.variants {
            if v.name != base.name {
                md.push_str(&format!(" {} |", v.name));
            }
        }
        md.push_str("\n|---|");
        md.push_str("---|");
        for v in &ds.variants {
            if v.name != base.name {
                md.push_str("---|");
            }
        }
        md.push('\n');

        for p in 0..n_passes {
            let bval = base.nonstop_per_pass[p];
            md.push_str(&format!("| {} | {:.0}ns", p + 1, bval));
            for v in &ds.variants {
                if v.name == base.name {
                    continue;
                }
                if p < v.nonstop_per_pass.len() {
                    let d = pct_delta(v.nonstop_per_pass[p], bval);
                    md.push_str(&format!(" | {:+.1}%", d));
                } else {
                    md.push_str(" | - ");
                }
            }
            md.push_str(" |\n");
        }

        md.push_str("\n**Autocorrelation (lag-1) per-pass series:**\n\n");
        md.push_str("| Variant | r₁ | note |\n|---|---|---|\n");
        for v in &ds.variants {
            let r = v.autocorrelation;
            let note = if r.abs() < 0.2 {
                "ok"
            } else if r > 0.5 {
                "HIGH+ (drift/warm-up)"
            } else if r < -0.5 {
                "HIGH- (thermal bounce)"
            } else if r > 0.2 {
                "moderate+"
            } else {
                "moderate-"
            };
            md.push_str(&format!("| {} | {:.3} | {} |\n", v.name, r, note));
        }

        md.push_str("\n**Consistency summary:**\n\n");
        for v in &ds.variants {
            if v.name == base.name {
                continue;
            }
            let mut wins = 0;
            let mut losses = 0;
            for p in 0..n_passes.min(v.nonstop_per_pass.len()) {
                let d = pct_delta(v.nonstop_per_pass[p], base.nonstop_per_pass[p]);
                if d < -0.1 {
                    wins += 1;
                } else if d > 0.1 {
                    losses += 1;
                }
            }
            let total = n_passes.min(v.nonstop_per_pass.len());
            md.push_str(&format!(
                "- **{}**: won {}/{}, lost {}/{}\n",
                v.name, wins, total, losses, total
            ));
        }
    }

    // ── Bridge overhead ──
    md.push_str("\n## Bridge overhead per variant\n\n");
    md.push_str("| Variant | mean bridge | algo mean | bridge % | flag |\n");
    md.push_str("|---|---|---|---|---|\n");
    for v in &ds.variants {
        let bridge_mean = v.bridge_all.mean;
        let algo_mean = v.algo_all.mean;
        let pct = if algo_mean > 0.0 {
            bridge_mean / algo_mean * 100.0
        } else {
            0.0
        };
        let flag = if pct > 5.0 { "HIGH" } else { "" };
        md.push_str(&format!(
            "| {} | {:.1}ns | {:.1}ns | {:.1}% | {} |\n",
            v.name, bridge_mean, algo_mean, pct, flag
        ));
    }

    // ── Quality scores (only if any variant has scores) ──
    let any_scores = ds.variants.iter().any(|v| !v.scores.is_empty());
    if any_scores {
        md.push_str("\n## Quality scores\n\n");
        md.push_str("| Variant | min | mean | median | max |\n");
        md.push_str("|---|---|---|---|---|\n");
        for v in &ds.variants {
            if v.scores.is_empty() {
                md.push_str(&format!("| {} | - | - | - | - |\n", v.name));
                continue;
            }
            let mut s = v.scores.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = s.len();
            let min = s[0];
            let max = s[n - 1];
            let mean = s.iter().sum::<f64>() / n as f64;
            let median = if n % 2 == 0 {
                (s[n / 2 - 1] + s[n / 2]) / 2.0
            } else {
                s[n / 2]
            };
            md.push_str(&format!(
                "| {} | {:.2} | {:.2} | {:.2} | {:.2} |\n",
                v.name, min, mean, median, max
            ));
        }
    }

    // ── Per-pattern breakdown (tagged routines) ──
    if !ds.tag_names.is_empty() {
        let tags: Vec<(u8, &str)> = ds
            .tag_names
            .iter()
            .map(|(&k, v)| (k, v.as_str()))
            .collect();

        md.push_str("\n## Per-pattern algo timing (ns)\n\n");
        md.push_str("| Variant |");
        for &(_, name) in &tags {
            md.push_str(&format!(" {} med | {} CV |", name, name));
        }
        md.push_str("\n|---|");
        for _ in &tags {
            md.push_str("---|---|");
        }
        md.push('\n');

        for v in &ds.variants {
            md.push_str(&format!("| {}", v.name));
            for &(tag, _) in &tags {
                if let Some(s) = v.algo_per_tag.get(&tag) {
                    let cv = if s.mean > 0.0 {
                        s.std_dev / s.mean * 100.0
                    } else {
                        0.0
                    };
                    md.push_str(&format!(" | {:.1} | {:.1}%", s.median, cv));
                } else {
                    md.push_str(" | - | -");
                }
            }
            md.push_str(" |\n");
        }
    }

    // ── Per-cooldown coefficient of variation ──
    let all_cds: Vec<u64> = base.e2e_per_cd.keys().cloned().collect();
    let nonzero_cds: Vec<u64> = all_cds.iter().cloned().filter(|&cd| cd > 0).collect();
    if !nonzero_cds.is_empty() {
        md.push_str("\n## Per-cooldown coefficient of variation (algo)\n\n");
        md.push_str("Coefficient of variation = std_dev / mean. Lower = more stable.\n\n");
        md.push_str("| Variant |");
        for cd in &nonzero_cds {
            md.push_str(&format!(" {}ms CV |", cd));
        }
        md.push_str("\n|---|");
        for _ in &nonzero_cds {
            md.push_str("---|");
        }
        md.push('\n');
        for v in &ds.variants {
            md.push_str(&format!("| {}", v.name));
            for cd in &nonzero_cds {
                if let Some(s) = v.algo_per_cd.get(cd) {
                    let cv = if s.mean > 0.0 {
                        s.std_dev / s.mean * 100.0
                    } else {
                        0.0
                    };
                    md.push_str(&format!(" | {:.1}%", cv));
                } else {
                    md.push_str(" | -");
                }
            }
            md.push_str(" |\n");
        }
    }

    // ── ASCII histogram of algo_ns distribution per variant ──
    md.push_str("\n## Distribution (algo ns)\n\n");
    md.push_str("```\n");
    let hist_bins = 20;
    let hist_width = 40;
    for v in &ds.variants {
        let vals: Vec<f64> = v.keyed_algo.iter().map(|&(_, _, _, ns)| ns).collect();
        if vals.is_empty() {
            continue;
        }
        let lo = v.algo_all.best_20pct;
        let hi = v.algo_all.worst_20pct;
        if hi <= lo {
            continue;
        }

        let bin_width = (hi - lo) / hist_bins as f64;
        let mut bins = vec![0u32; hist_bins];
        let mut below = 0u32;
        let mut above = 0u32;
        for &x in &vals {
            if x < lo {
                below += 1;
            } else if x >= hi {
                above += 1;
            } else {
                let b = ((x - lo) / bin_width) as usize;
                bins[b.min(hist_bins - 1)] += 1;
            }
        }
        let max_count = *bins.iter().max().unwrap_or(&1).max(&1);

        md.push_str(&format!(
            "{} (n={}, range {:.1}-{:.1} ns)\n",
            v.name, vals.len(), lo, hi
        ));
        for (i, &count) in bins.iter().enumerate() {
            let bar_len = (count as f64 / max_count as f64 * hist_width as f64) as usize;
            let edge = lo + i as f64 * bin_width;
            md.push_str(&format!("  {:7.1} |{}\n", edge, "#".repeat(bar_len)));
        }
        if below > 0 || above > 0 {
            md.push_str(&format!("  ({} below, {} above range)\n", below, above));
        }
        md.push('\n');
    }
    md.push_str("```\n");

    // ── Automated diagnostics ──
    let mut anomalies: Vec<String> = Vec::new();

    for v in &ds.variants {
        let s = &v.algo_all;
        if s.count < 2 {
            continue;
        }

        let cv = if s.mean > 0.0 {
            s.std_dev / s.mean * 100.0
        } else {
            0.0
        };
        if cv > 20.0 {
            anomalies.push(format!(
                "**{}**: CV={:.1}% (high variance, measurements may be unstable)",
                v.name, cv
            ));
        }

        if s.best_20pct > 0.0 && s.worst_20pct / s.best_20pct > 3.0 {
            anomalies.push(format!(
                "**{}**: worst_20/best_20 = {:.1}x (possible bimodal distribution)",
                v.name,
                s.worst_20pct / s.best_20pct
            ));
        }

        if v.autocorrelation > 0.5 {
            anomalies.push(format!(
                "**{}**: autocorrelation={:.2} (measurement drift or warm-up artifact)",
                v.name, v.autocorrelation
            ));
        }

        if v.bridge_all.median > 0.0 && s.median > 0.0 {
            let bridge_pct = v.bridge_all.median / s.median * 100.0;
            if bridge_pct > 10.0 {
                anomalies.push(format!(
                    "**{}**: bridge={:.1}% of algo (FFI overhead may distort results)",
                    v.name, bridge_pct
                ));
            }
        }
    }

    if !anomalies.is_empty() {
        md.push_str("\n## Diagnostics\n\n");
        for a in &anomalies {
            md.push_str(&format!("- {}\n", a));
        }
    }

    md
}
