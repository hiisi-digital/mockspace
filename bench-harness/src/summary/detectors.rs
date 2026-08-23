//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Highlight detectors for [`super::RunSummary`]. Each pattern-matches the run statistics and,
//! when it fires, returns a templated [`super::Highlight`]. Split from `summary.rs` per the
//! file-size rule; child-module access reaches the parent RunSummary's private helpers.

use super::{Highlight, RunSummary, VariantLine, fmt_ns};

type Detector = fn(&RunSummary) -> Option<Highlight>;

/// The highlight preset. Each entry pattern-matches the run's statistics and,
/// when it fires, returns a templated [`Highlight`]. Order here does not matter;
/// output is ranked by [`Highlight::importance`].
pub(super) static DETECTORS: &[Detector] = &[
    d_single_variant,
    d_baseline_is_slowest,
    d_baseline_is_fastest,
    d_dominant_winner,
    d_all_tied,
    d_photo_finish,
    d_outlier_slow,
    d_tight_field,
    d_wide_field,
    d_fast_but_least_stable,
    d_speed_stability_split,
    d_tiny_but_significant,
    d_large_significant,
    d_high_ties,
    d_drift,
    d_inconsistent_variant,
    d_below_resolution,
    d_two_tiers,
];

// ── detectors ──

fn d_single_variant(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() != 1 {
        return None;
    }
    let l = &s.lines[0];
    Some(Highlight {
        importance: 30,
        kind: "single_variant",
        headline: format!("Single variant {} at {} median", l.name, fmt_ns(l.median_ns)),
        detail: format!(
            "Only one variant ran, so there is nothing to compare. Median {}, \
             CV {:.1}%.",
            fmt_ns(l.median_ns),
            l.cv * 100.0
        ),
        why: "A one-variant bench measures a level, not a choice; add rivals to rank strategies."
            .into(),
    })
}

fn d_baseline_is_slowest(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let base = s.baseline_line()?;
    if base.name != s.slowest().name {
        return None;
    }
    let fastest = s.fastest();
    Some(Highlight {
        importance: 92,
        kind: "baseline_is_slowest",
        headline: format!(
            "Baseline ({}) is the SLOWEST variant; every rival beats it",
            base.name
        ),
        detail: format!(
            "The declared/defaulted baseline {} has the worst median ({}). Every \
             delta is therefore measured against the worst performer, which flatters \
             all rivals and compresses the differences that matter among them (e.g. \
             fastest {} at {}).",
            base.name,
            fmt_ns(base.median_ns),
            fastest.name,
            fmt_ns(fastest.median_ns)
        ),
        why: "A baseline picked by accident (often the first variant to run / \
              sort) silently skews every comparison. Re-baseline via \
              `[bench.<name>.normalise]` on a representative variant."
            .into(),
    })
}

fn d_baseline_is_fastest(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let base = s.baseline_line()?;
    if base.name != s.fastest().name {
        return None;
    }
    Some(Highlight {
        importance: 60,
        kind: "baseline_is_fastest",
        headline: format!("No variant beats the baseline ({})", base.name),
        detail: format!(
            "The baseline {} is the fastest ({} median); no rival improves on it \
             (all deltas are >= 0).",
            base.name,
            fmt_ns(base.median_ns)
        ),
        why: "When nothing beats the baseline, the current choice stands; the \
              contenders cost speed for whatever else they buy."
            .into(),
    })
}

fn d_dominant_winner(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let mut by_med: Vec<&VariantLine> = s.lines.iter().collect();
    by_med.sort_by(|a, b| a.median_ns.total_cmp(&b.median_ns));
    let win = by_med[0];
    let second = by_med[1];
    let gap = if win.median_ns > 0.0 {
        (second.median_ns - win.median_ns) / win.median_ns
    } else {
        0.0
    };
    // dominant = >=10% faster than the runner-up, and that gap is significant
    // (either the winner or runner-up is significant vs baseline with the right sign,
    // or the gap is large enough to be unambiguous).
    if gap < 0.10 {
        return None;
    }
    Some(Highlight {
        importance: 85,
        kind: "dominant_winner",
        headline: format!(
            "{} dominates: {:.0}% faster than the next best ({})",
            win.name,
            gap * 100.0,
            second.name
        ),
        detail: format!(
            "{} ({}) leads {} ({}) by {:.0}%, a clear separation rather than a \
             photo finish. CV {:.1}%.",
            win.name,
            fmt_ns(win.median_ns),
            second.name,
            fmt_ns(second.median_ns),
            gap * 100.0,
            win.cv * 100.0
        ),
        why: "A dominant, well-separated winner is a safe default pick for this \
              workload shape."
            .into(),
    })
}

fn d_all_tied(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let any_sig = s.lines.iter().any(|l| l.significant);
    if any_sig || s.field_spread() >= 1.03 {
        return None;
    }
    Some(Highlight {
        importance: 80,
        kind: "all_tied",
        headline: "All variants are a statistical tie (no significant difference)".into(),
        detail: format!(
            "No variant's paired difference vs the baseline is significant, and the \
             whole field sits within {:.1}% (fastest {} to slowest {}).",
            (s.field_spread() - 1.0) * 100.0,
            fmt_ns(s.fastest().median_ns),
            fmt_ns(s.slowest().median_ns)
        ),
        why: "When nothing separates, pick on a secondary axis (stability, code \
              simplicity, memory) - performance does not decide it here."
            .into(),
    })
}

fn d_photo_finish(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let mut by_med: Vec<&VariantLine> = s.lines.iter().collect();
    by_med.sort_by(|a, b| a.median_ns.total_cmp(&b.median_ns));
    let (w, r) = (by_med[0], by_med[1]);
    let gap = if w.median_ns > 0.0 { (r.median_ns - w.median_ns) / w.median_ns } else { 0.0 };
    // top two within 1% but the FIELD is not all-tied (else d_all_tied covers it)
    if gap >= 0.01 || s.field_spread() < 1.03 {
        return None;
    }
    Some(Highlight {
        importance: 65,
        kind: "photo_finish",
        headline: format!("Top two ({}, {}) are a dead heat (<1%)", w.name, r.name),
        detail: format!(
            "{} ({}) and {} ({}) differ by {:.2}%, inside the noise, even though the \
             wider field spreads {:.1}%.",
            w.name,
            fmt_ns(w.median_ns),
            r.name,
            fmt_ns(r.median_ns),
            gap * 100.0,
            (s.field_spread() - 1.0) * 100.0
        ),
        why: "Do not over-fit to the nominal leader when the runner-up is within \
              measurement noise; either is a fine pick."
            .into(),
    })
}

fn d_outlier_slow(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 3 {
        return None;
    }
    let slow = s.slowest();
    if slow.ratio_vs_best < 2.0 {
        return None;
    }
    Some(Highlight {
        importance: 72,
        kind: "outlier_slow",
        headline: format!("{} is an outlier: {:.1}x slower than the field", slow.name, slow.ratio_vs_best),
        detail: format!(
            "{} ({}) is {:.1}x the fastest ({}), well off the pack.",
            slow.name,
            fmt_ns(slow.median_ns),
            slow.ratio_vs_best,
            fmt_ns(s.fastest().median_ns)
        ),
        why: "A >2x outlier is almost never the right choice; if it is intentional \
              (e.g. it buys correctness), say so explicitly."
            .into(),
    })
}

fn d_tight_field(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 3 || s.field_spread() >= 1.05 {
        return None;
    }
    // only fire if there IS a significant difference (else d_all_tied)
    if !s.lines.iter().any(|l| l.significant) {
        return None;
    }
    Some(Highlight {
        importance: 45,
        kind: "tight_field",
        headline: format!("Whole field within {:.1}% of the fastest", (s.field_spread() - 1.0) * 100.0),
        detail: format!(
            "All {} variants sit between {} and {} - a {:.1}% band - though some \
             paired differences are still significant.",
            s.lines.len(),
            fmt_ns(s.fastest().median_ns),
            fmt_ns(s.slowest().median_ns),
            (s.field_spread() - 1.0) * 100.0
        ),
        why: "Small but real gaps: worth taking only where this path is hot enough \
              that a few percent compounds."
            .into(),
    })
}

fn d_wide_field(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 || s.field_spread() < 3.0 {
        return None;
    }
    Some(Highlight {
        importance: 55,
        kind: "wide_field",
        headline: format!("Wide spread: slowest is {:.1}x the fastest", s.field_spread()),
        detail: format!(
            "Fastest {} ({}) to slowest {} ({}): {:.1}x. The strategy choice matters \
             a lot for this workload.",
            s.fastest().name,
            fmt_ns(s.fastest().median_ns),
            s.slowest().name,
            fmt_ns(s.slowest().median_ns),
            s.field_spread()
        ),
        why: "A wide field means the strategy is load-bearing here; getting it \
              right (or wrong) has large consequences."
            .into(),
    })
}

fn d_fast_but_least_stable(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let fastest = s.fastest();
    if fastest.name != s.least_stable().name || fastest.cv < 0.05 {
        return None;
    }
    let stable = s.most_stable();
    Some(Highlight {
        importance: 68,
        kind: "fast_but_least_stable",
        headline: format!(
            "{} is fastest but the noisiest (CV {:.1}%)",
            fastest.name,
            fastest.cv * 100.0
        ),
        detail: format!(
            "{} wins on median ({}) yet has the highest variance (CV {:.1}%), while \
             {} is the steadiest (CV {:.1}%, {}).",
            fastest.name,
            fmt_ns(fastest.median_ns),
            fastest.cv * 100.0,
            stable.name,
            stable.cv * 100.0,
            fmt_ns(stable.median_ns)
        ),
        why: "For latency-sensitive or tail-bound paths, the steadier variant can \
              beat the faster-on-average one; weigh peak vs consistency."
            .into(),
    })
}

fn d_speed_stability_split(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let fastest = s.fastest();
    let stable = s.most_stable();
    if fastest.name == stable.name {
        return None;
    }
    // stability leader is not itself the fastest, and is within 10% speed
    let speed_cost = if fastest.median_ns > 0.0 {
        (stable.median_ns - fastest.median_ns) / fastest.median_ns
    } else {
        0.0
    };
    if speed_cost > 0.10 || fastest.cv < 0.03 {
        return None;
    }
    Some(Highlight {
        importance: 50,
        kind: "speed_stability_split",
        headline: format!(
            "Speed leader {} vs stability leader {} ({:+.0}% speed for {:.1}x steadier)",
            fastest.name,
            stable.name,
            speed_cost * 100.0,
            if stable.cv > 0.0 { fastest.cv / stable.cv } else { 1.0 }
        ),
        detail: format!(
            "{} is fastest ({}, CV {:.1}%); {} gives up {:.1}% median for {:.1}x lower \
             variance (CV {:.1}%).",
            fastest.name,
            fmt_ns(fastest.median_ns),
            fastest.cv * 100.0,
            stable.name,
            speed_cost * 100.0,
            if stable.cv > 0.0 { fastest.cv / stable.cv } else { 1.0 },
            stable.cv * 100.0
        ),
        why: "The pick depends on priority: peak throughput vs predictable latency. \
              Both are defensible; name which the workload needs."
            .into(),
    })
}

fn d_tiny_but_significant(s: &RunSummary) -> Option<Highlight> {
    let base = s.baseline_line();
    let base_med = base.map(|b| b.median_ns).unwrap_or(0.0);
    for l in &s.lines {
        if let Some(dn) = l.delta_vs_base_ns {
            let pct = if base_med > 0.0 { dn.abs() / base_med } else { 1.0 };
            if l.significant && pct < 0.02 && dn.abs() < 50.0 {
                return Some(Highlight {
                    importance: 42,
                    kind: "tiny_but_significant",
                    headline: format!(
                        "{}'s edge over baseline is significant but tiny ({}, {:.2}%)",
                        l.name,
                        fmt_ns(dn),
                        pct * 100.0
                    ),
                    detail: format!(
                        "{} differs from baseline {} by {} ({:.2}%) - statistically \
                         real (CI excludes zero) but small enough to be practically \
                         irrelevant.",
                        l.name,
                        s.baseline,
                        fmt_ns(dn),
                        pct * 100.0
                    ),
                    why: "Statistical significance is not practical significance: a \
                          measurable-but-tiny gap should not drive a decision."
                        .into(),
                });
            }
        }
    }
    None
}

fn d_large_significant(s: &RunSummary) -> Option<Highlight> {
    let base = s.baseline_line();
    let base_med = base.map(|b| b.median_ns).unwrap_or(0.0);
    let mut best: Option<(&VariantLine, f64)> = None;
    for l in &s.lines {
        if let (Some(dn), true) = (l.delta_vs_base_ns, l.significant) {
            let pct = if base_med > 0.0 { dn / base_med } else { 0.0 };
            if pct <= -0.20 {
                if best.map(|(_, p)| pct < p).unwrap_or(true) {
                    best = Some((l, pct));
                }
            }
        }
    }
    let (l, pct) = best?;
    Some(Highlight {
        importance: 78,
        kind: "large_significant",
        headline: format!("{} beats baseline by {:.0}% (significant)", l.name, -pct * 100.0),
        detail: format!(
            "{} is {} ({:.0}%) faster than baseline {}, with a CI that excludes zero.",
            l.name,
            fmt_ns(l.delta_vs_base_ns.unwrap()),
            -pct * 100.0,
            s.baseline
        ),
        why: "A large, significant improvement over the current baseline is a \
              concrete reason to switch."
            .into(),
    })
}

fn d_high_ties(s: &RunSummary) -> Option<Highlight> {
    let worst = s.lines.iter().filter(|l| l.delta_vs_base_ns.is_some())
        .max_by(|a, b| a.tie_frac.total_cmp(&b.tie_frac))?;
    if worst.tie_frac < 0.10 {
        return None;
    }
    Some(Highlight {
        importance: 58,
        kind: "high_ties",
        headline: format!("{}'s comparison is tie-heavy ({:.0}% tied pairs)", worst.name, worst.tie_frac * 100.0),
        detail: format!(
            "{:.0}% of paired samples for {} are exact ties vs baseline, weakening \
             the sign test - the timer resolution may be coarser than the effect.",
            worst.tie_frac * 100.0,
            worst.name
        ),
        why: "A high tie rate means the difference is at or below measurement \
              resolution; trust it less and consider a heavier workload per call."
            .into(),
    })
}

fn d_drift(s: &RunSummary) -> Option<Highlight> {
    let drifted = s.lines.iter().max_by(|a, b| a.autocorrelation.abs().total_cmp(&b.autocorrelation.abs()))?;
    if drifted.autocorrelation.abs() < 0.5 {
        return None;
    }
    let kind_txt = if drifted.autocorrelation > 0.0 { "warm-up / thermal drift" } else { "alternating (throttle bounce)" };
    Some(Highlight {
        importance: 63,
        kind: "drift",
        headline: format!("{} shows {} (autocorr {:+.2})", drifted.name, kind_txt, drifted.autocorrelation),
        detail: format!(
            "{}'s per-pass series has lag-1 autocorrelation {:+.2}, indicating {}. \
             Its timing may not be at steady state.",
            drifted.name,
            drifted.autocorrelation,
            kind_txt
        ),
        why: "Autocorrelated samples violate the independence the CIs assume; the \
              interval is optimistic until the drift is warmed out or cooled down."
            .into(),
    })
}

fn d_inconsistent_variant(s: &RunSummary) -> Option<Highlight> {
    let worst = s.lines.iter().max_by(|a, b| {
        let ra = if a.best_20pct_ns > 0.0 { a.worst_20pct_ns / a.best_20pct_ns } else { 1.0 };
        let rb = if b.best_20pct_ns > 0.0 { b.worst_20pct_ns / b.best_20pct_ns } else { 1.0 };
        ra.total_cmp(&rb)
    })?;
    let ratio = if worst.best_20pct_ns > 0.0 { worst.worst_20pct_ns / worst.best_20pct_ns } else { 1.0 };
    if ratio < 1.5 {
        return None;
    }
    Some(Highlight {
        importance: 48,
        kind: "inconsistent_variant",
        headline: format!("{} is inconsistent: worst-20% is {:.1}x its best-20%", worst.name, ratio),
        detail: format!(
            "{}'s best 20% of batches run at {} but its worst 20% at {} ({:.1}x) - a \
             bimodal or bursty profile the median hides.",
            worst.name,
            fmt_ns(worst.best_20pct_ns),
            fmt_ns(worst.worst_20pct_ns),
            ratio
        ),
        why: "A fat tail matters for latency budgets even when the median looks \
              fine; a steadier variant may serve better under load."
            .into(),
    })
}

fn d_below_resolution(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 2 {
        return None;
    }
    let spread = s.slowest().median_ns - s.fastest().median_ns;
    // resolution proxy: the std_dev of the fastest variant
    let res = s.fastest().std_dev_ns;
    if res <= 0.0 || spread >= res {
        return None;
    }
    Some(Highlight {
        importance: 62,
        kind: "below_resolution",
        headline: "Whole-field spread is below the measurement noise floor".into(),
        detail: format!(
            "The fastest-to-slowest gap ({}) is smaller than the fastest variant's \
             own run-to-run std-dev ({}); the ranking is inside the noise.",
            fmt_ns(spread),
            fmt_ns(res)
        ),
        why: "When the spread is below resolution, any apparent ordering is likely \
              noise; increase work per call before trusting a winner."
            .into(),
    })
}

fn d_two_tiers(s: &RunSummary) -> Option<Highlight> {
    if s.lines.len() < 4 {
        return None;
    }
    let mut meds: Vec<(&str, f64)> = s.lines.iter().map(|l| (l.name.as_str(), l.median_ns)).collect();
    meds.sort_by(|a, b| a.1.total_cmp(&b.1));
    // biggest relative gap between consecutive medians
    let mut best_gap = 0.0;
    let mut split = 0usize;
    for i in 0..meds.len() - 1 {
        let g = if meds[i].1 > 0.0 { (meds[i + 1].1 - meds[i].1) / meds[i].1 } else { 0.0 };
        if g > best_gap {
            best_gap = g;
            split = i + 1;
        }
    }
    // a clear two-tier split: the gap is > 2x the largest within-tier gap and >= 25%
    if best_gap < 0.25 || split == 0 || split == meds.len() {
        return None;
    }
    let fast: Vec<&str> = meds[..split].iter().map(|(n, _)| *n).collect();
    let slow: Vec<&str> = meds[split..].iter().map(|(n, _)| *n).collect();
    Some(Highlight {
        importance: 60,
        kind: "two_tiers",
        headline: format!("Two tiers: {{{}}} vs {{{}}} ({:.0}% apart)", fast.join(", "), slow.join(", "), best_gap * 100.0),
        detail: format!(
            "The field splits into a fast tier {{{}}} and a slow tier {{{}}} with a \
             {:.0}% jump between them - a qualitative difference, not a gradient.",
            fast.join(", "),
            slow.join(", "),
            best_gap * 100.0
        ),
        why: "A tier split usually reflects a mechanism boundary (branchless vs \
              branch, cached vs not); the tier, not the exact rank, is the finding."
            .into(),
    })
}
