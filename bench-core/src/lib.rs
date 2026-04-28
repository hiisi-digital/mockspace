//! Mockspace bench framework. Core primitives for variant-comparison benches.
//!
//! Consumer-side mockspace repos use this crate to define `Routine` impls
//! (one per algorithm-under-test) and multiple variant impls per Routine.
//! The `RoutineBridge` + `routine_bridge!` macro turn a monomorphised
//! Routine into a byte-level FFI surface so the harness (shipped
//! separately) can load each variant as a dylib and time them in
//! isolation.
//!
//! `no_std` by default; opt into `std` for `RoutineBridge`,
//! `routine_bridge!`, time-conversion helpers, and macOS P-core pinning.
//!
//! ## Origin
//!
//! Framework was originally written by orgrinrt under MIT in
//! `polka-dots/mock/benches/bench-core/` (the substrate that drove
//! arvo's strategy-marker design). Relicensed by the original author
//! under MPL-2.0 for the mockspace stack. Lifted here so every
//! mockspace consumer gets the canonical surface instead of re-rolling
//! it.

#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod counter;

/// Defines WHAT is computed. All variants implement this contract;
/// the harness compares them on identical inputs.
///
/// `Input` and `Output` must be `Copy + flat layout` for the byte-level
/// FFI bridge. `repr(C)` cannot be enforced via trait bounds, but the
/// `routine_bridge!` macro verifies non-zero size at compile time.
pub trait Routine {
    /// Input type. Must be `Copy` and flat (no pointers / no references).
    type Input: Copy;

    /// Output type. Must be comparable for cross-variant validation.
    type Output: PartialEq + core::fmt::Debug + Copy;

    /// Build input deterministically from a seed.
    fn build_input(seed: u64) -> Self::Input;

    /// Validate that an output is structurally correct (not just
    /// consistent across variants). Default: no structural check; the
    /// harness still does cross-variant byte comparison unless
    /// `outputs_may_differ` is true.
    fn validate_output(_input: &Self::Input, _output: &Self::Output) -> Result<(), &'static str> {
        Ok(())
    }

    /// Score an output for quality comparison across variants.
    /// Lower = better. None means "no quality metric for this routine".
    /// Examples: number of colours (graph colouring), bandwidth (RCM).
    fn score_output(_input: &Self::Input, _output: &Self::Output) -> Option<f64> {
        None
    }

    /// Label for the quality metric (for instance "colours", "bandwidth").
    fn score_label() -> Option<&'static str> {
        None
    }

    /// Whether different variants may produce different valid outputs.
    /// false (default): harness also does cross-variant byte comparison.
    /// true: only `validate_output` is checked (for instance graph colouring).
    fn outputs_may_differ() -> bool {
        false
    }

    /// Maximum relative error for cross-variant comparison of
    /// floating-point outputs. When `Some(eps)`, validation compares
    /// outputs element-wise using relative error instead of byte-exact
    /// equality. Each f64 pair `(a, b)` passes if
    /// `|a - b| <= eps * max(|a|, |b|, 1e-15)`.
    ///
    /// When None (default): byte-exact comparison (or no cross-variant
    /// comparison if `outputs_may_differ` is true).
    fn max_relative_error() -> Option<f64> {
        None
    }

    /// Number of logical operations per call. When > 0, the harness
    /// reports throughput (ops/ns, ops/us) alongside latency.
    /// Examples: edge count (graph), nonzero count (SpMV).
    fn ops_per_call(_input: &Self::Input) -> u64 {
        0
    }

    /// Maximum expected per-call time in microseconds for a given size N.
    /// If a worker's batch mean exceeds this, it aborts early and reports
    /// TIMEOUT. Prevents exponential-time variants from stalling the
    /// entire bench run at large N.
    fn max_call_us(_n: usize) -> Option<u64> {
        None
    }

    /// Classify the input generated from this seed into a tag for
    /// per-pattern breakdown in reports. None means the routine has a
    /// single input type (for instance RCM, colouring).
    ///
    /// For SpMV: upper bits of seed select sparsity pattern (banded,
    /// random, block-diagonal, power-law). The tag is a u8 index.
    /// Analysis groups by tag for per-pattern timing comparisons.
    fn input_tag(_seed: u64) -> Option<(&'static str, u8)> {
        None
    }

    // ── Byte-level bridge for the dylib harness (std only) ──

    /// Size of the output type in bytes.
    fn output_size() -> usize {
        core::mem::size_of::<Self::Output>()
    }

    /// Serialise a built input to bytes.
    #[cfg(feature = "std")]
    fn build_input_bytes(seed: u64) -> std::vec::Vec<u8> {
        let input = Self::build_input(seed);
        let ptr = &input as *const Self::Input as *const u8;
        let size = core::mem::size_of::<Self::Input>();
        debug_assert_eq!(
            ptr as usize % core::mem::align_of::<Self::Input>(),
            0,
            "build_input_bytes: input pointer is not aligned for Self::Input"
        );
        unsafe { core::slice::from_raw_parts(ptr, size) }.to_vec()
    }

    /// Multi-dimensional quality scores for Pareto analysis.
    /// Each entry is `(label, value)` where lower = better.
    /// Default: empty.
    #[cfg(feature = "std")]
    fn score_dimensions(
        _input: &Self::Input,
        _output: &Self::Output,
    ) -> std::vec::Vec<(&'static str, f64)> {
        std::vec::Vec::new()
    }

    /// Score dimensions from raw bytes.
    #[cfg(feature = "std")]
    fn score_dimensions_bytes(
        input_bytes: &[u8],
        output_bytes: &[u8],
    ) -> std::vec::Vec<(&'static str, f64)> {
        debug_assert_eq!(
            input_bytes.as_ptr() as usize % core::mem::align_of::<Self::Input>(),
            0,
            "score_dimensions_bytes: input_bytes pointer is not aligned for Self::Input"
        );
        debug_assert_eq!(
            output_bytes.as_ptr() as usize % core::mem::align_of::<Self::Output>(),
            0,
            "score_dimensions_bytes: output_bytes pointer is not aligned for Self::Output"
        );
        unsafe {
            let input = &*(input_bytes.as_ptr() as *const Self::Input);
            let output = &*(output_bytes.as_ptr() as *const Self::Output);
            Self::score_dimensions(input, output)
        }
    }

    /// Validate output from raw bytes. Casts and delegates to validate_output.
    #[cfg(feature = "std")]
    fn validate_output_bytes(
        input_bytes: &[u8],
        output_bytes: &[u8],
    ) -> Result<(), std::string::String> {
        debug_assert_eq!(
            input_bytes.as_ptr() as usize % core::mem::align_of::<Self::Input>(),
            0,
            "validate_output_bytes: input_bytes pointer is not aligned for Self::Input"
        );
        debug_assert_eq!(
            output_bytes.as_ptr() as usize % core::mem::align_of::<Self::Output>(),
            0,
            "validate_output_bytes: output_bytes pointer is not aligned for Self::Output"
        );
        unsafe {
            let input = &*(input_bytes.as_ptr() as *const Self::Input);
            let output = &*(output_bytes.as_ptr() as *const Self::Output);
            Self::validate_output(input, output).map_err(std::string::String::from)
        }
    }

    /// Score output from raw bytes. Casts and delegates to score_output.
    #[cfg(feature = "std")]
    fn score_output_bytes(input_bytes: &[u8], output_bytes: &[u8]) -> Option<f64> {
        debug_assert_eq!(
            input_bytes.as_ptr() as usize % core::mem::align_of::<Self::Input>(),
            0,
            "score_output_bytes: input_bytes pointer is not aligned for Self::Input"
        );
        debug_assert_eq!(
            output_bytes.as_ptr() as usize % core::mem::align_of::<Self::Output>(),
            0,
            "score_output_bytes: output_bytes pointer is not aligned for Self::Output"
        );
        unsafe {
            let input = &*(input_bytes.as_ptr() as *const Self::Input);
            let output = &*(output_bytes.as_ptr() as *const Self::Output);
            Self::score_output(input, output)
        }
    }

    /// Compare two output byte slices using relative error tolerance.
    #[cfg(feature = "std")]
    fn compare_outputs_approx(
        a: &[u8],
        b: &[u8],
        epsilon: f64,
    ) -> Result<(), std::string::String> {
        if a.len() != b.len() {
            return Err(std::format!(
                "output size mismatch: {} vs {}",
                a.len(),
                b.len()
            ));
        }
        let n = a.len() / core::mem::size_of::<f64>();
        let a_f64 = unsafe { core::slice::from_raw_parts(a.as_ptr() as *const f64, n) };
        let b_f64 = unsafe { core::slice::from_raw_parts(b.as_ptr() as *const f64, n) };
        for i in 0..n {
            let va = a_f64[i];
            let vb = b_f64[i];
            let denom = va.abs().max(vb.abs()).max(1e-15);
            let rel_err = (va - vb).abs() / denom;
            if rel_err > epsilon {
                return Err(std::format!(
                    "element [{}]: {:.6e} vs {:.6e} (rel_err={:.2e}, eps={:.2e})",
                    i, va, vb, rel_err, epsilon
                ));
            }
        }
        Ok(())
    }

    /// Ops per call from raw input bytes.
    #[cfg(feature = "std")]
    fn ops_per_call_bytes(input_bytes: &[u8]) -> u64 {
        debug_assert_eq!(
            input_bytes.as_ptr() as usize % core::mem::align_of::<Self::Input>(),
            0,
            "ops_per_call_bytes: input_bytes pointer is not aligned for Self::Input"
        );
        unsafe {
            let input = &*(input_bytes.as_ptr() as *const Self::Input);
            Self::ops_per_call(input)
        }
    }
}

/// Byte-level bridge for one monomorphised Routine. Captures all the
/// fn pointers the harness needs without knowing the concrete
/// Input/Output types. Built via `routine_bridge!`.
#[cfg(feature = "std")]
pub struct RoutineBridge {
    pub input_builder: fn(u64) -> std::vec::Vec<u8>,
    pub output_size: usize,
    pub validator: fn(&[u8], &[u8]) -> Result<(), std::string::String>,
    pub outputs_may_differ: bool,
    pub max_relative_error: Option<f64>,
    pub approx_comparator: fn(&[u8], &[u8], f64) -> Result<(), std::string::String>,
    pub scorer: fn(&[u8], &[u8]) -> Option<f64>,
    pub score_label: Option<&'static str>,
    pub ops_per_call: fn(&[u8]) -> u64,
    pub max_call_us: fn(usize) -> Option<u64>,
    pub input_tagger: Option<fn(u64) -> (std::string::String, u8)>,
}

/// Build a RoutineBridge from a monomorphised Routine type.
#[cfg(feature = "std")]
#[macro_export]
macro_rules! routine_bridge {
    ($R:ty) => {{
        const _: () = {
            assert!(
                core::mem::size_of::<<$R as $crate::Routine>::Input>() > 0,
                "Routine::Input must be non-zero-sized for byte-level FFI bridge"
            );
            assert!(
                core::mem::size_of::<<$R as $crate::Routine>::Output>() > 0,
                "Routine::Output must be non-zero-sized for byte-level FFI bridge"
            );
        };
        $crate::RoutineBridge {
            input_builder: <$R as $crate::Routine>::build_input_bytes,
            output_size: <$R as $crate::Routine>::output_size(),
            validator: <$R as $crate::Routine>::validate_output_bytes,
            outputs_may_differ: <$R as $crate::Routine>::outputs_may_differ(),
            max_relative_error: <$R as $crate::Routine>::max_relative_error(),
            approx_comparator: <$R as $crate::Routine>::compare_outputs_approx,
            scorer: <$R as $crate::Routine>::score_output_bytes,
            score_label: <$R as $crate::Routine>::score_label(),
            ops_per_call: <$R as $crate::Routine>::ops_per_call_bytes,
            max_call_us: <$R as $crate::Routine>::max_call_us,
            input_tagger: {
                fn __tagger(seed: u64) -> Option<(std::string::String, u8)> {
                    <$R as $crate::Routine>::input_tag(seed)
                        .map(|(name, idx)| (std::string::String::from(name), idx))
                }
                if <$R as $crate::Routine>::input_tag(0).is_some() {
                    Some(|seed| __tagger(seed).unwrap())
                } else {
                    None
                }
            },
        }
    }};
}

/// Timing result returned across the dylib boundary.
///
/// `run_ticks` is the duration of the run-block in hardware counter
/// ticks. The harness subtracts this from its own measurement to
/// compute bridge overhead.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfiBenchCall {
    pub run_ticks: u64,
}

/// Function signature exported by each variant dylib.
/// `n` is a size parameter for multi-N dispatch (consumer-defined; the
/// harness passes whatever sizes the consumer registered).
pub type BenchEntryFn = unsafe extern "C" fn(
    input: *const u8,
    output: *mut u8,
    n: usize,
) -> FfiBenchCall;

/// Name accessor exported by each variant dylib.
pub type BenchNameFn = extern "C" fn() -> *const u8;

/// ABI hash for version checking on dylib load.
pub type AbiHashFn = extern "C" fn() -> u64;

/// Compute the ABI hash at compile time. FNV-1a over the FfiBenchCall
/// layout. Variants compile-in this hash at build time; on load, the
/// harness checks the hash to detect ABI drift.
pub const fn abi_hash() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let size = core::mem::size_of::<FfiBenchCall>() as u64;
    h ^= size;
    h = h.wrapping_mul(0x100000001b3);
    h ^= 1u64; // field count: run_ticks
    h = h.wrapping_mul(0x100000001b3);
    h ^= 8u64; // run_ticks size
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// Time a block and return FfiBenchCall. Use inside `#[bench_variant]`
/// functions (or hand-written variant entry points):
///
/// ```ignore
/// fn variant<const N: usize>(input: &Input<N>, output: &mut Output<N>) -> FfiBenchCall {
///     mockspace_bench_core::timed! {
///         setup { /* untimed setup */ }
///         run { algorithm::<N>(input, output); }
///         /* untimed teardown after run block */
///     }
/// }
/// ```
#[macro_export]
macro_rules! timed {
    ( $( $tokens:tt )* ) => {
        $crate::__bench_expand_body!( $( $tokens )* )
    };
}

/// Internal: expand the body. The body is a sequence of tt tokens that
/// contains exactly one `run { ... }` block. Tokens before are setup
/// (not timed); tokens after are teardown (not timed). A tt-muncher
/// accumulates setup, then on `run` emits the timed block.
#[macro_export]
macro_rules! __bench_expand_body {
    ( @setup [ $( $setup:tt )* ] run { $( $run:tt )* } $( $teardown:tt )* ) => {{
        $( $setup )*
        let __start = $crate::counter::read_counter();
        $( $run )*
        let __end = $crate::counter::read_counter();
        $( $teardown )*
        $crate::FfiBenchCall { run_ticks: __end - __start }
    }};

    ( @setup [ $( $setup:tt )* ] $next:tt $( $rest:tt )* ) => {
        $crate::__bench_expand_body!( @setup [ $( $setup )* $next ] $( $rest )* )
    };

    ( $( $tokens:tt )* ) => {
        $crate::__bench_expand_body!( @setup [] $( $tokens )* )
    };
}
