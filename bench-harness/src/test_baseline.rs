//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A baseline per cell and the statistic that holds an outlier out of it.
//!
//! A cell is one test on one machine class in one flavour and profile; samples
//! of different cells are never pooled. The baseline is the median of the last
//! [`WINDOW`] clean samples, and a new sample is `Suspect` when its modified
//! z-score (median absolute deviation) passes [`Z_LIMIT`] or it is over
//! [`RATIO_LIMIT`] times the median. A cell with fewer than [`WARMING`] clean
//! samples flags nothing: there is not yet a spread to judge against.
//!
//! This only judges. The verdict store that keeps a `Suspect` out of the
//! aggregate until it is cleared or confirmed is a later step.

use crate::analysis::median;

/// How many of the latest clean samples make the baseline.
pub const WINDOW: usize = 20;
/// Clean samples a cell needs before anything in it can be flagged.
pub const WARMING: usize = 8;
/// The modified z-score past which a sample is suspect.
pub const Z_LIMIT: f64 = 6.0;
/// A sample over this many times the baseline median is suspect.
pub const RATIO_LIMIT: f64 = 3.0;

/// What the statistic says of one new sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Judgement {
    /// Too few clean samples to judge against.
    Warming,
    Clean,
    Suspect,
}

/// Judge `sample` against the cell's clean history, oldest first.
///
/// Only the last [`WINDOW`] entries count. A history with no spread (every
/// sample equal, so a zero deviation) falls back to the ratio alone, since a
/// z-score over zero is no statistic.
#[must_use]
pub fn judge(clean: &[f64], sample: f64) -> Judgement {
    if clean.len() < WARMING {
        return Judgement::Warming;
    }
    let window = &clean[clean.len().saturating_sub(WINDOW) ..];
    let med = median(window);
    let devs: Vec<f64> = window.iter().map(|x| (x - med).abs()).collect();
    let mad = median(&devs);
    let by_ratio = med > 0.0 && sample > RATIO_LIMIT * med;
    let by_z = mad > 0.0 && 0.6745 * (sample - med).abs() / mad > Z_LIMIT;
    if by_ratio || by_z { Judgement::Suspect } else { Judgement::Clean }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JITTER: [f64; 10] = [1.00, 1.05, 0.97, 1.02, 0.99, 1.04, 0.96, 1.01, 1.03, 0.98];

    /// Before the cell has eight clean samples nothing is flagged, even a 100x one.
    #[test]
    fn a_cell_under_eight_clean_samples_flags_nothing() {
        assert_eq!(judge(&JITTER[.. 5], 100.0), Judgement::Warming);
        assert_eq!(judge(&JITTER[.. 7], 100.0), Judgement::Warming);
        assert_eq!(judge(&JITTER[.. 8], 100.0), Judgement::Suspect);
    }

    #[test]
    fn ordinary_jitter_is_clean() {
        for s in [0.95, 1.0, 1.06, 1.10] {
            assert_eq!(judge(&JITTER, s), Judgement::Clean, "{s}");
        }
    }

    #[test]
    fn a_spike_past_the_ratio_or_the_z_limit_is_suspect() {
        assert_eq!(judge(&JITTER, 3.5), Judgement::Suspect);
        assert_eq!(judge(&JITTER, 1.5), Judgement::Suspect); // z: far outside a 0.03 spread
    }

    #[test]
    fn a_history_with_no_spread_falls_back_to_the_ratio() {
        let flat = [2.0; 10];
        assert_eq!(judge(&flat, 2.0), Judgement::Clean);
        assert_eq!(judge(&flat, 2.5), Judgement::Clean);
        assert_eq!(judge(&flat, 6.5), Judgement::Suspect);
    }

    /// Only the latest window is the baseline: an old era does not hold it.
    #[test]
    fn only_the_last_window_makes_the_baseline() {
        let mut h = vec![10.0; 30];
        h.extend(JITTER.iter().cycle().take(WINDOW));
        assert_eq!(judge(&h, 1.0), Judgement::Clean);
    }
}
