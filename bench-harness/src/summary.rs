//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Procedural result highlights, shared by the findings report and the driver's
//! end-of-run stdout summary.
//!
//! The harness already computes rich per-variant statistics ([`crate::analysis`]);
//! plain tables leave the reader to eyeball what matters. This module derives a
//! [`RunSummary`] from a [`DataSet`] and runs a library of *highlight detectors*
//! over it: each detector pattern-matches the statistics and, when it fires,
//! emits a [`Highlight`] with a one-line headline plus (for the report) the
//! evidence of how it composed and a procedurally-named reason it matters. The
//! fired highlights are ranked by importance.
//!
//! Both consumers share this one engine:
//! - the report renders every highlight in full (headline + detail + why) so the
//!   findings *analyse* the data rather than only display it;
//! - the stdout summary renders just the headlines, states the baseline, and
//!   ends by directing the reader to the full report.
//!
//! Adding a highlight is one function pushed into [`DETECTORS`]. The baseline is
//! always named in the output so an accidentally-chosen baseline (e.g. the first
//! variant alphabetically) is caught rather than silently skewing every delta.

use crate::analysis::{DataSet, compare};

/// Human-readable nanoseconds.
fn fmt_ns(ns: f64) -> String {
    let a = ns.abs();
    if a >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if a >= 1_000.0 {
        format!("{:.2} us", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

/// Per-variant computed line: the numbers the detectors and renderers read.
#[derive(Clone, Debug)]
pub struct VariantLine {
    pub name:             String,
    pub median_ns:        f64,
    pub mean_ns:          f64,
    pub std_dev_ns:       f64,
    /// Coefficient of variation (std_dev / median): scale-free stability.
    pub cv:               f64,
    pub best_20pct_ns:    f64,
    pub worst_20pct_ns:   f64,
    /// median / fastest-variant-median (>= 1.0).
    pub ratio_vs_best:    f64,
    /// Paired `median(variant - baseline)` in ns. `None` for the baseline row.
    pub delta_vs_base_ns: Option<f64>,
    /// Paired-difference CI excludes zero (statistically significant).
    pub significant:      bool,
    /// Tied-pair fraction from the sign test (0..1); high weakens the result.
    pub tie_frac:         f64,
    /// Lag-1 autocorrelation of the per-pass series (drift / thermal bounce).
    pub autocorrelation:  f64,
}

/// One computed highlight. `headline` is the one-liner both consumers show;
/// `detail` and `why` expand it in the report only.
#[derive(Clone, Debug)]
pub struct Highlight {
    /// Ranking weight; higher shows first. Roughly: 90+ correctness/validity
    /// concerns, 70-89 decisive performance verdicts, 40-69 notable structure,
    /// < 40 minor colour.
    pub importance: u8,
    /// Stable slug for the pattern (dedup / testing).
    pub kind:       &'static str,
    /// The one-liner (stdout + report headline).
    pub headline:   String,
    /// Report-only: how the headline composes (the numbers and comparison).
    pub detail:     String,
    /// Report-only: procedurally-named reason this matters.
    pub why:        String,
}

/// A computed, presentation-ready summary of one bench run at one size.
#[derive(Clone, Debug)]
pub struct RunSummary {
    pub title:    String,
    pub baseline: String,
    pub lines:    Vec<VariantLine>,
}

impl RunSummary {
    fn fastest(&self) -> &VariantLine {
        self.lines
            .iter()
            .min_by(|a, b| a.median_ns.total_cmp(&b.median_ns))
            .unwrap()
    }

    fn slowest(&self) -> &VariantLine {
        self.lines
            .iter()
            .max_by(|a, b| a.median_ns.total_cmp(&b.median_ns))
            .unwrap()
    }

    fn most_stable(&self) -> &VariantLine {
        self.lines
            .iter()
            .min_by(|a, b| a.cv.total_cmp(&b.cv))
            .unwrap()
    }

    fn least_stable(&self) -> &VariantLine {
        self.lines
            .iter()
            .max_by(|a, b| a.cv.total_cmp(&b.cv))
            .unwrap()
    }

    fn baseline_line(&self) -> Option<&VariantLine> {
        self.lines.iter().find(|l| l.name == self.baseline)
    }

    fn field_spread(&self) -> f64 {
        let f = self.fastest().median_ns;
        if f > 0.0 { self.slowest().median_ns / f } else { 1.0 }
    }

    /// Run every detector, keep the ones that fire, ranked most-important first.
    pub fn highlights(&self) -> Vec<Highlight> {
        let mut hs: Vec<Highlight> = DETECTORS.iter().filter_map(|d| d(self)).collect();
        hs.sort_by(|a, b| b.importance.cmp(&a.importance));
        hs
    }

    /// Compact terminal block: baseline, a per-variant line, the highlight
    /// headlines, then a pointer to the full report.
    pub fn render_terminal(&self, report_path: &str) -> String {
        let mut s = String::new();
        s.push_str(&format!("  {} (baseline: {})\n", self.title, self.baseline));
        for l in &self.lines {
            let d = match l.delta_vs_base_ns {
                Some(dn) => {
                    format!(
                        "  Δbase {}{}",
                        fmt_ns(dn),
                        if l.significant { " *" } else { "" }
                    )
                },
                None => "  (baseline)".to_string(),
            };
            s.push_str(&format!(
                "    {:<24} {:>10}  {:>5.2}x  cv {:>4.1}%{}\n",
                l.name,
                fmt_ns(l.median_ns),
                l.ratio_vs_best,
                l.cv * 100.0,
                d
            ));
        }
        let hs = self.highlights();
        if hs.is_empty() {
            s.push_str("    (no notable statistical pattern detected)\n");
        } else {
            for h in hs.iter().take(6) {
                s.push_str(&format!("    * {}\n", h.headline));
            }
        }
        s.push_str(&format!(
            "    -> Read the full report for details: {report_path}\n"
        ));
        s
    }

    /// Report section: baseline note + every highlight expanded (headline,
    /// how it composed, why it matters).
    pub fn render_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("## Highlights\n\n");
        md.push_str(&format!(
            "Baseline for all deltas below: **{}**. (Deltas are paired \
             `variant - baseline` medians; `*` marks a CI that excludes zero.)\n\n",
            self.baseline
        ));
        let hs = self.highlights();
        if hs.is_empty() {
            md.push_str(
                "No notable statistical pattern fired: the variants do not \
                 separate meaningfully on this run.\n\n",
            );
            return md;
        }
        for h in &hs {
            md.push_str(&format!("### {}\n\n", h.headline));
            md.push_str(&format!("{}\n\n", h.detail));
            md.push_str(&format!("_Why it matters:_ {}\n\n", h.why));
        }
        md
    }
}

/// Pair a variant's keyed samples against the baseline's, group by group.
///
/// `(run, pass, cooldown_ms)` is not a key over a variant's samples: the
/// orchestrator emits `runs_per_pass / batch_size` batch lines per worker
/// invocation and every one of them carries that same triple, ten of them on
/// the shipped defaults. Both sides are sorted by the triple, so a merge join
/// pairs the i-th batch of a group with the i-th batch of the same group and
/// resynchronises at the next group boundary when one side is short. A map
/// keyed on the triple instead keeps one baseline batch per group and pairs
/// every variant batch against that single value, which strips the baseline's
/// own variance out of the differences and leaves the paired bootstrap
/// measuring one arm.
pub(crate) fn pair_keyed(
    variant: &[(usize, usize, u64, f64)],
    base: &[(usize, usize, u64, f64)],
) -> (Vec<f64>, Vec<f64>) {
    let mut vv = Vec::new();
    let mut bv = Vec::new();
    let (mut vi, mut bi) = (0usize, 0usize);
    while vi < variant.len() && bi < base.len() {
        let (vrun, vpass, vcd, vval) = variant[vi];
        let (brun, bpass, bcd, bval) = base[bi];
        match (vrun, vpass, vcd).cmp(&(brun, bpass, bcd)) {
            std::cmp::Ordering::Equal => {
                vv.push(vval);
                bv.push(bval);
                vi += 1;
                bi += 1;
            },
            std::cmp::Ordering::Less => vi += 1,
            std::cmp::Ordering::Greater => bi += 1,
        }
    }
    (vv, bv)
}

/// Build a [`RunSummary`] from a [`DataSet`]. Paired deltas are computed against
/// the dataset's baseline variant (declare it via `[bench.<name>.normalise]`),
/// using the keyed samples so pairing is by `(run, pass, cooldown)`.
pub fn summarise(ds: &DataSet, title: &str, seed: u64) -> RunSummary {
    let base = ds.baseline();
    let best_median = ds
        .variants
        .iter()
        .map(|v| v.algo_all.median)
        .fold(f64::INFINITY, f64::min);
    let mut lines = Vec::new();
    for v in &ds.variants {
        let (delta, sig, tie_frac) = if v.name == base.name {
            (None, false, 0.0)
        } else {
            let (vv, bv) = pair_keyed(&v.keyed_algo, &base.keyed_algo);
            if vv.is_empty() {
                (None, false, 0.0)
            } else {
                let cmp = compare(&vv, &bv, seed);
                let tf = cmp.ties as f64 / vv.len() as f64;
                (Some(cmp.median_diff_ns), cmp.significant, tf)
            }
        };
        let median = v.algo_all.median;
        lines.push(VariantLine {
            name:             v.name.clone(),
            median_ns:        median,
            mean_ns:          v.algo_all.mean,
            std_dev_ns:       v.algo_all.std_dev,
            cv:               if median > 0.0 { v.algo_all.std_dev / median } else { 0.0 },
            best_20pct_ns:    v.algo_all.best_20pct,
            worst_20pct_ns:   v.algo_all.worst_20pct,
            ratio_vs_best:    if best_median > 0.0 { median / best_median } else { 1.0 },
            delta_vs_base_ns: delta,
            significant:      sig,
            tie_frac:         tie_frac,
            autocorrelation:  v.autocorrelation,
        });
    }
    RunSummary {
        title: title.to_string(),
        baseline: base.name.clone(),
        lines,
    }
}

mod detectors;
use detectors::DETECTORS;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::DataSet;
    use crate::sample::Sample;

    fn s(variant: &str, run: usize, pass: usize, cd: u64, batch: usize, algo: f64) -> Sample {
        Sample {
            run,
            pass,
            cooldown_ms: cd,
            mode: "warm".into(),
            variant: variant.into(),
            e2e_ns: algo,
            algo_ns: algo,
            bridge_ns: 0.0,
            batch_idx: batch,
            batch_count: 1,
            score: None,
            input_tag: None,
            instructions: 0,
            cycles: 0,
            setup_ns: 0.0,
            first_ns: 0.0,
            digest: 0,
        }
    }

    /// The orchestrator emits `runs_per_pass / batch_size` samples per
    /// `(run, pass, cooldown_ms)` (ten, on the shipped defaults), so that triple
    /// is not a unique key over a variant's samples. Pairing has to keep every
    /// baseline batch; a keyed lookup keeps one and reuses it, which strips the
    /// baseline's variance out of the paired differences and turns the paired
    /// bootstrap into a one-sided interval.
    ///
    /// The discriminator: two variants with the identical series inside one
    /// group. Every honest pairing gives a zero difference and a full tie count.
    #[test]
    fn pairing_keeps_every_baseline_batch_not_one_per_key() {
        let mut samples = Vec::new();
        for (b, v) in [10.0f64, 20.0, 30.0, 40.0].iter().enumerate() {
            samples.push(s("aaa_base", 1, 1, 0, b, *v));
            samples.push(s("bbb_rival", 1, 1, 0, b, *v));
        }
        let ds = DataSet::from_samples(&samples, "warm");
        let rs = summarise(&ds, "t", 0xFEED_BEEF);
        let rival = rs
            .lines
            .iter()
            .find(|l| l.name == "bbb_rival")
            .expect("rival line");

        assert_eq!(
            rival.delta_vs_base_ns,
            Some(0.0),
            "the rival ran the identical series as the baseline in the same \
             (run, pass, cooldown) group, so the paired median difference is 0"
        );
        assert!(
            !rival.significant,
            "identical series cannot differ significantly"
        );
        assert_eq!(
            rival.tie_frac, 1.0,
            "every pair is an exact tie; a tie fraction below 1 means the \
             pairing lost baseline samples"
        );
    }

    /// The same run, summarised twice, has to say the same thing. The report
    /// calls `summarise(ds, title, ds.meta.master_seed)` and the driver calls it
    /// with the config's seed, so a seed-dependent verdict is two verdicts.
    #[test]
    fn the_verdict_does_not_depend_on_the_bootstrap_seed() {
        let mut samples = Vec::new();
        for (b, (base, rival)) in
            [(100.0f64, 97.0f64), (101.0, 103.0), (99.0, 98.0), (102.0, 104.0)]
                .iter()
                .enumerate()
        {
            samples.push(s("aaa_base", 1, 1, 0, b, *base));
            samples.push(s("bbb_rival", 1, 1, 0, b, *rival));
        }
        let ds = DataSet::from_samples(&samples, "warm");
        let a = summarise(&ds, "t", 0);
        let b = summarise(&ds, "t", 0x9E37_79B9_7F4A_7C15);
        let sig_a: Vec<bool> = a.lines.iter().map(|l| l.significant).collect();
        let sig_b: Vec<bool> = b.lines.iter().map(|l| l.significant).collect();
        assert_eq!(
            sig_a, sig_b,
            "the significance verdict changed with the bootstrap seed alone"
        );
    }
}
