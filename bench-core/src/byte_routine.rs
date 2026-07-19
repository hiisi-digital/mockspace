//! Default Routine impl for byte-input / byte-output workloads.
//!
//! `ByteRoutine<IN, OUT, MAY_DIFFER>` is the canonical Routine for
//! benches whose input is "build N bytes from seed" and whose output
//! is "M bytes". Hash algos, encoders, fixed-shape transforms, etc.
//!
//! ## Usage
//!
//! Orchestrator side:
//!
//! ```ignore
//! use mockspace_bench_core::{routine_bridge, ByteRoutine, RoutineSpec};
//!
//! // FNV1a vs xxHash3 over 64-byte inputs, 8-byte digests, algos
//! // produce different digests for the same input.
//! type HashRoutine64 = ByteRoutine<64, 8, true>;
//! let bridge = routine_bridge!(HashRoutine64);
//! ```
//!
//! Variant side: see `mockspace_bench_macro::bench_variant` (typed
//! form). The macro takes a `name` + `sizes` and reads input/output
//! types from the function signature, so variants do not need to
//! reference `ByteRoutine` at all.
//!
//! ## When to use this versus a custom Routine
//!
//! Use `ByteRoutine` when the workload is exactly "build N bytes,
//! compute M bytes". Override at the const-generic boundary: each
//! `(IN, OUT, MAY_DIFFER)` triple is a separate monomorphisation, so
//! a single bench at multiple sizes uses one bridge per size (the
//! orchestrator already iterates per size).
//!
//! Use a custom Routine impl when:
//! - Input is not a flat byte array (struct, enum, sparse layout).
//! - Output validation needs structural checks beyond byte equality.
//! - `ops_per_call` is not "input length" (matrix nnz, edge count,
//!   etc.).
//! - Quality scoring is meaningful (graph colouring count, RCM
//!   bandwidth, etc.).

use crate::Routine;

/// Default Routine impl for `Input = [u8; IN]`, `Output = [u8; OUT]`.
///
/// `MAY_DIFFER = false` (default): variants must produce byte-exact
/// matching outputs for the same input. Used when comparing different
/// implementations of the same function (SIMD vs scalar).
///
/// `MAY_DIFFER = true`: variants are independent algorithms. Each
/// must be internally deterministic per seed; outputs across variants
/// are not compared. Used when comparing distinct algorithms (FNV1a
/// vs xxHash3, two graph colouring heuristics, etc.).
#[derive(Debug, Clone, Copy)]
pub struct ByteRoutine<const IN: usize, const OUT: usize, const MAY_DIFFER: bool = false>;

impl<const IN: usize, const OUT: usize, const MAY_DIFFER: bool> Routine
    for ByteRoutine<IN, OUT, MAY_DIFFER>
{
    type Input = [u8; IN];
    type Output = [u8; OUT];

    /// Seed-driven content using a SplitMix64-style mixer over 8-byte
    /// chunks. Reproducible per seed; content-uniform; cheap to build.
    fn build_input(seed: u64) -> [u8; IN] {
        let mut buf = [0u8; IN];
        let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15);
        for chunk in buf.chunks_mut(8) {
            x ^= x >> 30;
            x = x.wrapping_mul(0xBF58476D1CE4E5B9);
            let bytes = x.to_le_bytes();
            chunk.copy_from_slice(&bytes[.. chunk.len()]);
        }
        buf
    }

    /// Heap-filling override of the bridge path: fill a Vec of
    /// exactly IN bytes directly, never materialising `[u8; IN]` on
    /// the stack. This removes the practical ceiling on IN (a
    /// multi-megabyte input would otherwise be a stack overflow
    /// hazard at build time) while staying fully const-generic: the
    /// size is still a compile-time constant per monomorphisation.
    #[cfg(feature = "std")]
    fn build_input_bytes(seed: u64) -> std::vec::Vec<u8> {
        let mut buf = std::vec![0u8; IN];
        let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15);
        for chunk in buf.chunks_mut(8) {
            x ^= x >> 30;
            x = x.wrapping_mul(0xBF58476D1CE4E5B9);
            let bytes = x.to_le_bytes();
            chunk.copy_from_slice(&bytes[.. chunk.len()]);
        }
        buf
    }

    fn ops_per_call(_input: &Self::Input) -> u64 {
        IN as u64
    }

    fn outputs_may_differ() -> bool {
        MAY_DIFFER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_input_is_deterministic_per_seed() {
        let a = ByteRoutine::<64, 8, false>::build_input(42);
        let b = ByteRoutine::<64, 8, false>::build_input(42);
        assert_eq!(a, b);
    }

    #[test]
    fn build_input_differs_across_seeds() {
        let a = ByteRoutine::<64, 8, false>::build_input(1);
        let b = ByteRoutine::<64, 8, false>::build_input(2);
        assert_ne!(a, b);
    }

    #[test]
    fn ops_per_call_is_input_length() {
        let buf = ByteRoutine::<256, 8, false>::build_input(0);
        assert_eq!(ByteRoutine::<256, 8, false>::ops_per_call(&buf), 256);
    }

    #[test]
    fn may_differ_flag_propagates() {
        assert!(!ByteRoutine::<64, 8, false>::outputs_may_differ());
        assert!(ByteRoutine::<64, 8, true>::outputs_may_differ());
    }

    #[cfg(feature = "std")]
    #[test]
    fn heap_build_matches_typed_build() {
        let typed = ByteRoutine::<64, 8, false>::build_input(7);
        let heaped = <ByteRoutine<64, 8, false> as Routine>::build_input_bytes(7);
        assert_eq!(&typed[..], &heaped[..]);
    }

    #[cfg(feature = "std")]
    #[test]
    fn dispatch_macro_declared_sizes_only() {
        let d = crate::byte_routine_dispatch!(out = 8, sizes = [64, 1024]);
        assert!((d.dispatch)(64, false).is_some());
        assert!((d.dispatch)(1024, true).is_some());
        assert!(
            (d.dispatch)(512, false).is_none(),
            "undeclared size rejects"
        );
        assert_eq!(d.sizes, &[64, 1024]);
    }

    #[test]
    fn timed_accepts_plain_and_wrapped_setup() {
        fn plain() -> crate::FfiBenchCall {
            crate::timed! {
                let x = 2u64;
                run { core::hint::black_box(x * 2); }
            }
        }
        fn wrapped() -> crate::FfiBenchCall {
            crate::timed! {
                setup {
                    let x = 2u64;
                }
                run { core::hint::black_box(x * 2); }
                let _teardown = x;
            }
        }
        let _ = plain();
        let _ = wrapped();
    }

    #[test]
    fn small_sizes_work() {
        let _ = ByteRoutine::<1, 1, false>::build_input(0);
        let _ = ByteRoutine::<8, 8, false>::build_input(0);
        let _ = ByteRoutine::<16, 8, false>::build_input(0);
    }
}
