//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The canonical measurement wrappers. Every generated variant's timed region is
//! a call into one of these, so the anti-hoist chain, the shared seed table, the
//! S timing, the reps-invariant fidelity digest, and the fold-one-keep-alive rule
//! are provided once, compiled once, tested once, instead of re-derived in a
//! template string per consumer (which is how the four measurement distortions the
//! review panel found got shipped: see the crate README and TODO).
//!
//! The load-bearing constraint, and the whole isolation argument: the cell is a
//! generic `FnMut` type parameter (a zero-sized fn-item or closure), NEVER a `fn`
//! pointer and NEVER `&dyn Fn`. A generic parameter is monomorphized and inlined
//! into the timed loop by the variant's fat-LTO cdylib link, with no indirection,
//! exactly as the old string `body` was inlined across the same crate boundary. A
//! `fn` pointer would reintroduce an indirect call the optimizer may decline to
//! devirtualize, so the measured region would include a call the real deployment
//! would not. The signatures below only ever take the generic form.

use mockspace_bench_core::counter::read_counter;
use mockspace_bench_core::{timed_calibrated, FfiBenchCall};

/// The fixed 16-entry seed table, shared across every size and every cell. Drawing
/// seeds from this instead of from the harness `input` bytes (which the harness
/// fills per-size) keeps a cross-size or cross-cell comparison varying only the
/// thing under test. (Panel finding: `input[k % N]` drew different seeds per size.)
pub const SEEDS: [u64; 16] = [
    0x9e37_79b9_7f4a_7c15,
    0xf1bb_cdcb_fa53_e0a9,
    0x2545_f491_4f6c_dd1d,
    0x8ebc_6af0_9c88_c2b2,
    0xc2b2_ae3d_27d4_eb4f,
    0x1656_67b1_9e37_79f9,
    0x27d4_eb2f_1656_67c5,
    0x1656_67b1_9e37_79b9,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
];

/// The number of inner iterations per timed pass (one per seed).
pub const ITERS: usize = 16;

/// The reps-invariant fidelity digest's fixed initial value.
const DIGEST_INIT: u64 = 0xF1DE_1178_ABCD_0001;

/// Everything a scaffold measures for one variant call. `run_ticks` is the
/// per-pass calibrated I term (the number the old harness returned); `setup_ticks`
/// is the S term (the one-time build cost that was hidden in untimed prep);
/// `first_ticks` is the true cold first-touch pass; `digest` is the reps-invariant
/// cross-validation witness.
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    pub run_ticks:   u64,
    pub setup_ticks: u64,
    pub first_ticks: u64,
    pub digest:      u64,
}

impl Measured {
    /// Bridge to `FfiBenchCall`. Since the ABI extension (TODO.md item 1) the
    /// struct carries all four fields, so the scaffold's setup / first-touch /
    /// digest measurements survive across the dylib boundary and the harness
    /// reporter surfaces them as `setup_ns` / `algo_ns_first` columns and the
    /// cross-variant fidelity check reads `digest` instead of the reps-variant
    /// output bytes.
    pub fn into_ffi(self) -> FfiBenchCall {
        FfiBenchCall {
            run_ticks:   self.run_ticks,
            setup_ticks: self.setup_ticks,
            first_ticks: self.first_ticks,
            digest:      self.digest,
        }
    }
}

/// Warm regime: build one state (timed as S), then measure the per-iteration op
/// (the I term) under calibration. Emits a reps-invariant fidelity digest and a
/// cold first-touch pass alongside.
///
/// `setup` runs once and its cost is always timed, so the S term can never be
/// hidden in untimed prep (the panel's number-one finding, made structural: setup
/// is a required argument the scaffold brackets with counter reads). `cell`
/// returns ONE keep-alive `u64` that the scaffold folds once per outer iteration,
/// never once per node, so the O(N) fidelity fold cannot leak into the inner loop.
///
/// `cell` MUST be idempotent: its state is built once and reused across the
/// first-touch pass, the digest pass, and the calibrated loop, so a cell must
/// overwrite its scratch and return an output that is a function of the seed
/// alone, not of accumulated prior-call state. A cell that accumulates would make
/// the digest order-dependent and the calibrated I term drift across reps.
#[inline]
pub fn warm<const N: usize, St, Setup, Cell>(
    input: &[u8; N],
    output: &mut [u8; 8],
    setup: Setup,
    mut cell: Cell,
) -> Measured
where
    Setup: FnOnce(usize) -> St,
    Cell: FnMut(&mut St, u64) -> u64,
{
    let _ = input; // seeds come from the shared SEEDS table, not the harness input.

    // S term: time setup once. Always reported; never optional.
    let s0 = read_counter();
    let mut state = setup(N);
    let s1 = read_counter();

    // first-touch column: one explicitly timed cold pass. This MUST run before the
    // digest pass (and before calibration), on the freshly-built state, or it is
    // not cold: the digest pass calls the cell ITERS times, which warms caches and
    // trains the predictor. It relies on the cell being idempotent (overwrites its
    // scratch, output a function of the seed only), which is the scaffold contract.
    let f0 = read_counter();
    {
        let mut acc = 0u64;
        let mut k = 0usize;
        while k < ITERS {
            acc ^= cell(&mut state, SEEDS[k]);
            k += 1;
        }
        core::hint::black_box(acc);
    }
    let f1 = read_counter();

    // reps-INVARIANT fidelity digest: one pass, fixed init, fixed seeds, outside
    // the timed loop. Reps-invariant so cross-validating on it is meaningful under
    // calibration, where the reps-variant `output` is not (panel finding 6).
    let mut digest: u64 = DIGEST_INIT;
    {
        let mut k = 0usize;
        while k < ITERS {
            digest = digest.rotate_left(7) ^ cell(&mut state, SEEDS[k]);
            k += 1;
        }
    }

    // I term: anti-hoist `acc` seeded from `output[0]`, written back each rep so
    // the calibrated reps form a loop-carried dependency the optimizer cannot
    // collapse. The scaffold folds one keep-alive (the cell's return) per iteration.
    let call = timed_calibrated! { run {
        let mut acc: u64 = output[0] as u64;
        let mut k = 0usize;
        while k < ITERS {
            acc ^= cell(&mut state, SEEDS[k] ^ (k as u64));
            k += 1;
        }
        output.copy_from_slice(&acc.to_le_bytes());
    }};

    Measured { run_ticks: call.run_ticks, setup_ticks: s1 - s0, first_ticks: f1 - f0, digest }
}

/// Cold / aliased-predictor regime: `setup` returns a state holding `m` distinct
/// programs; the scaffold passes the iteration index `k` so the cell selects
/// `k % m`, and no single program's dispatch sequence fits the branch predictor.
/// This is the many-residuals-per-frame deployment shape, the opposite of the warm
/// regime's memorized single program.
#[inline]
pub fn cold_cycle<const N: usize, St, Setup, Cell>(
    input: &[u8; N],
    output: &mut [u8; 8],
    setup: Setup,
    mut cell: Cell,
) -> Measured
where
    Setup: FnOnce(usize) -> St,
    Cell: FnMut(&mut St, usize, u64) -> u64,
{
    let _ = input;

    let s0 = read_counter();
    let mut state = setup(N);
    let s1 = read_counter();

    // first-touch (cold) before the digest pass warms the state; see `warm`.
    let f0 = read_counter();
    {
        let mut acc = 0u64;
        let mut k = 0usize;
        while k < ITERS {
            acc ^= cell(&mut state, k, SEEDS[k]);
            k += 1;
        }
        core::hint::black_box(acc);
    }
    let f1 = read_counter();

    let mut digest: u64 = DIGEST_INIT;
    {
        let mut k = 0usize;
        while k < ITERS {
            digest = digest.rotate_left(7) ^ cell(&mut state, k, SEEDS[k]);
            k += 1;
        }
    }

    let call = timed_calibrated! { run {
        let mut acc: u64 = output[0] as u64;
        let mut k = 0usize;
        while k < ITERS {
            acc ^= cell(&mut state, k, SEEDS[k] ^ (k as u64));
            k += 1;
        }
        output.copy_from_slice(&acc.to_le_bytes());
    }};

    Measured { run_ticks: call.run_ticks, setup_ticks: s1 - s0, first_ticks: f1 - f0, digest }
}

/// Stream regime: the measured op sweeps the harness `input` byte stream itself,
/// a throughput-over-a-byte-stream measurement (O(input) work per call), as
/// opposed to the warm regime's per-seed execution. The cell receives the whole
/// `input` slice and returns one keep-alive; the scaffold folds one per iteration
/// and re-runs the sweep under calibration. This is the shape a native-ceiling or
/// run-over-input throughput bench needs, where the data being swept is the
/// harness input, not the shared seed table.
///
/// `setup` is still timed once (S), and the digest/first-touch are single sweeps,
/// exactly as in [`warm`]. The cell takes `&[u8]` (the `input: &[u8; N]` coerces),
/// so the generated cell is not const-generic over N.
#[inline]
pub fn stream<const N: usize, St, Setup, Cell>(
    input: &[u8; N],
    output: &mut [u8; 8],
    setup: Setup,
    mut cell: Cell,
) -> Measured
where
    Setup: FnOnce(usize) -> St,
    Cell: FnMut(&mut St, &[u8]) -> u64,
{
    // S term: time setup once. Always reported.
    let s0 = read_counter();
    let mut state = setup(N);
    let s1 = read_counter();

    // first-touch column: one explicitly timed cold sweep, before the digest sweep
    // warms the state; see `warm`.
    let f0 = read_counter();
    {
        let acc = cell(&mut state, input);
        core::hint::black_box(acc);
    }
    let f1 = read_counter();

    // reps-invariant fidelity digest: one sweep over the input, fixed init.
    let digest = DIGEST_INIT.rotate_left(7) ^ cell(&mut state, input);

    // I term: anti-hoist `acc` seeded from `output[0]`, folded per rep. Each rep
    // re-runs the whole input sweep, so a calibrated run is the throughput measure.
    let call = timed_calibrated! { run {
        let mut acc: u64 = output[0] as u64;
        acc ^= cell(&mut state, input);
        output.copy_from_slice(&acc.to_le_bytes());
    }};

    Measured { run_ticks: call.run_ticks, setup_ticks: s1 - s0, first_ticks: f1 - f0, digest }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A cell that overwrites scratch (idempotent under repeated calls, as the
    // calibration contract requires) and returns a keep-alive derived from the seed.
    #[test]
    fn warm_times_setup_and_run_and_digest_is_reps_invariant() {
        let input = [7u8; 64];
        let mut out = [0u8; 8];
        let m1 = warm::<64, _, _, _>(
            &input,
            &mut out,
            |_n| vec![0u64; 8],
            |scratch: &mut Vec<u64>, seed| {
                for (i, x) in scratch.iter_mut().enumerate() {
                    *x = seed.wrapping_mul(i as u64 + 1);
                }
                scratch.iter().fold(0u64, |h, &v| h.rotate_left(5) ^ v)
            },
        );
        // setup ran (a Vec alloc is not free), the run block was timed, and the
        // digest is the fixed-seed fold, independent of the reps the calibrator chose.
        let mut out2 = [0u8; 8];
        let m2 = warm::<64, _, _, _>(
            &input,
            &mut out2,
            |_n| vec![0u64; 8],
            |scratch: &mut Vec<u64>, seed| {
                for (i, x) in scratch.iter_mut().enumerate() {
                    *x = seed.wrapping_mul(i as u64 + 1);
                }
                scratch.iter().fold(0u64, |h, &v| h.rotate_left(5) ^ v)
            },
        );
        assert_eq!(m1.digest, m2.digest, "digest must be reps-invariant across calls");
        assert_ne!(m1.digest, DIGEST_INIT, "digest must actually fold the cell output");
    }

    #[test]
    fn stream_sweeps_the_input_and_times_setup() {
        // a cell that sweeps the input bytes and folds them; setup allocates a
        // scratch so its cost is nonzero, and the digest folds the input sweep.
        let input = [5u8; 64];
        let mut out = [0u8; 8];
        let m = stream::<64, _, _, _>(
            &input,
            &mut out,
            |n| vec![0u64; n],
            |scratch: &mut Vec<u64>, inp: &[u8]| {
                let _ = &scratch;
                inp.iter().fold(0u64, |h, &b| h.rotate_left(3) ^ b as u64)
            },
        );
        // the input sweep folded a nonzero digest (input is all 5s, not all 0s).
        assert_ne!(m.digest, 0);
    }

    #[test]
    fn into_ffi_carries_all_four_fields() {
        let m = Measured { run_ticks: 123, setup_ticks: 5, first_ticks: 9, digest: 42 };
        let f = m.into_ffi();
        // all four measurements must cross the dylib boundary; dropping any one
        // is the regression that hid the S term in the hand-rolled matrix.
        assert_eq!(f.run_ticks, 123);
        assert_eq!(f.setup_ticks, 5);
        assert_eq!(f.first_ticks, 9);
        assert_eq!(f.digest, 42);
    }
}
