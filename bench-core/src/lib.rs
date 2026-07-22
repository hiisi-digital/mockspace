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

//!
//! ## Threading contract
//!
//! The worker pins only its own thread to a performance core
//! (`pin_to_perf_cores` uses self-thread QoS and affinity calls);
//! threads a variant spawns inside its run block are NOT pinned and
//! do not inherit the QoS class. Timing uses free-running counters,
//! so a run block that spawns work and joins it is measured correctly
//! as wall-clock elapsed time for the block. A bench whose variants
//! spawn threads declares `threaded = true` in its manifest section,
//! which disables the coordinating thread's self-pin (pinning only
//! the coordinator skews a threaded workload). What per-thread
//! counters would report for spawned threads is out of scope: only
//! the calling thread is instrumented.
//!
//! ## Heavyweight per-process state
//!
//! Each worker is a dedicated subprocess per (variant, mode, pass),
//! so a variant may cache expensive state (a built scheduler, a
//! loaded dataset) in a process-local static keyed by `n`: the cache
//! is naturally per-variant and per-run, and at least one warmup call
//! always executes before the preflight probe and the timed batches,
//! so lazy initialisation lands in untimed territory. Build such
//! state on first call; never rebuild it per timed call.

#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod byte_routine;
pub mod counter;

pub use byte_routine::ByteRoutine;

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
    #[must_use]
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
    fn compare_outputs_approx(a: &[u8], b: &[u8], epsilon: f64) -> Result<(), std::string::String> {
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
        for i in 0 .. n {
            let va = a_f64[i];
            let vb = b_f64[i];
            let denom = va.abs().max(vb.abs()).max(1e-15);
            let rel_err = (va - vb).abs() / denom;
            if rel_err > epsilon {
                return Err(std::format!(
                    "element [{}]: {:.6e} vs {:.6e} (rel_err={:.2e}, eps={:.2e})",
                    i,
                    va,
                    vb,
                    rel_err,
                    epsilon
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
    pub input_builder:      fn(u64) -> std::vec::Vec<u8>,
    pub output_size:        usize,
    pub validator:          fn(&[u8], &[u8]) -> Result<(), std::string::String>,
    pub outputs_may_differ: bool,
    pub max_relative_error: Option<f64>,
    pub approx_comparator:  fn(&[u8], &[u8], f64) -> Result<(), std::string::String>,
    pub scorer:             fn(&[u8], &[u8]) -> Option<f64>,
    pub score_label:        Option<&'static str>,
    pub ops_per_call:       fn(&[u8]) -> u64,
    pub max_call_us:        fn(usize) -> Option<u64>,
    pub input_tagger:       Option<fn(u64) -> (std::string::String, u8)>,
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
            input_builder:      <$R as $crate::Routine>::build_input_bytes,
            output_size:        <$R as $crate::Routine>::output_size(),
            validator:          <$R as $crate::Routine>::validate_output_bytes,
            outputs_may_differ: <$R as $crate::Routine>::outputs_may_differ(),
            max_relative_error: <$R as $crate::Routine>::max_relative_error(),
            approx_comparator:  <$R as $crate::Routine>::compare_outputs_approx,
            scorer:             <$R as $crate::Routine>::score_output_bytes,
            score_label:        <$R as $crate::Routine>::score_label(),
            ops_per_call:       <$R as $crate::Routine>::ops_per_call_bytes,
            max_call_us:        <$R as $crate::Routine>::max_call_us,
            input_tagger:       {
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

/// Generate the `n -> RoutineSpec` dispatch for [`ByteRoutine`]
/// benches from ONE declared const sizes list.
///
/// Every size stays its own monomorphisation (the input shape is a
/// compile-time constant per N, which is the framework's stability
/// guarantee: the set of inputs a bench accepts is strictly
/// controlled and statically known). What this macro removes is the
/// hand-managed match in every consumer driver: declare the sizes
/// once, get the whole dispatch, and a manifest size outside the
/// list is a targeted error naming this declaration instead of a
/// silent gap.
///
/// ```ignore
/// let dispatch = byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 16384]);
/// // dispatch(n, may_differ) -> Option<RoutineBridge>
/// ```
#[cfg(feature = "std")]
#[macro_export]
macro_rules! byte_routine_dispatch {
    (out = $out:literal, sizes = [ $( $n:literal ),* $(,)? ]) => {{
        fn __dispatch(n: usize, may_differ: bool) -> Option<$crate::RoutineBridge> {
            if may_differ {
                match n {
                    $( $n => Some($crate::routine_bridge!($crate::ByteRoutine<$n, $out, true>)), )*
                    _ => None,
                }
            } else {
                match n {
                    $( $n => Some($crate::routine_bridge!($crate::ByteRoutine<$n, $out, false>)), )*
                    _ => None,
                }
            }
        }
        /// The declared sizes, for diagnostics.
        const __SIZES: &[usize] = &[$( $n ),*];
        ($crate::ByteDispatch { dispatch: __dispatch, sizes: __SIZES })
    }};
}

/// A generated size dispatch: see [`byte_routine_dispatch!`].
#[cfg(feature = "std")]
#[derive(Clone, Copy)]
pub struct ByteDispatch {
    /// Resolve `(n, may_differ)` to a monomorphised bridge, or `None`
    /// when `n` is not in the declared list.
    pub dispatch: fn(usize, bool) -> Option<RoutineBridge>,
    /// The declared sizes (for error messages).
    pub sizes:    &'static [usize],
}

/// Timing result returned across the dylib boundary.
///
/// `run_ticks` is the duration of the run-block in hardware counter
/// ticks. The harness subtracts this from its own measurement to
/// compute bridge overhead.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FfiBenchCall {
    pub run_ticks: u64,
}

/// Function signature exported by each variant dylib.
/// `n` is a size parameter for multi-N dispatch (consumer-defined; the
/// harness passes whatever sizes the consumer registered).
pub type BenchEntryFn =
    unsafe extern "C" fn(input: *const u8, output: *mut u8, n: usize) -> FfiBenchCall;

/// Name accessor exported by each variant dylib.
pub type BenchNameFn = extern "C" fn() -> *const u8;

/// ABI hash for version checking on dylib load.
pub type AbiHashFn = extern "C" fn() -> u64;

/// Compute the ABI hash at compile time. FNV-1a over the FfiBenchCall
/// layout. Variants compile-in this hash at build time; on load, the
/// harness checks the hash to detect ABI drift.
#[must_use]
pub const fn abi_hash() -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    let size = core::mem::size_of::<FfiBenchCall>() as u64;
    h ^= size;
    h = h.wrapping_mul(0x100000001B3);
    h ^= 1u64; // field count: run_ticks
    h = h.wrapping_mul(0x100000001B3);
    h ^= 8u64; // run_ticks size
    h = h.wrapping_mul(0x100000001B3);
    h
}

/// The counter-quantization floor: the minimum tick span a timed region should
/// cover so the counter's `+/- 1` tick quantization is a negligible fraction of
/// the measurement. 2048 ticks holds the quantization error under about 0.05%.
pub const CALIBRATION_FLOOR_TICKS: u64 = 2048;

/// How many repetitions of a run make its total exceed `floor_ticks`, given one
/// run measured `probe_ticks`. So the per-run time (total / reps) is measured
/// above the counter's resolution instead of being dominated by quantization.
///
/// Always at least 1 (a run already above the floor needs no repetition), and
/// capped at 2^20 so a near-zero probe cannot request an unbounded loop. This is
/// the duration-floor calibration a variant applies itself (via
/// [`timed_calibrated!`]), because the timed region lives inside the variant and
/// only the variant can repeat it.
#[must_use]
pub const fn calibrate_reps(probe_ticks: u64, floor_ticks: u64) -> u64 {
    if probe_ticks >= floor_ticks {
        return 1;
    }
    let p = if probe_ticks == 0 { 1 } else { probe_ticks };
    let reps = floor_ticks.div_ceil(p);
    if reps > (1 << 20) { 1 << 20 } else { reps }
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
    // The documented `setup { ... }` wrapper: unwrap its tokens into
    // the setup accumulator. Without this rule the wrapper leaked
    // into the generated code verbatim, where `setup { ... }` parses
    // as a struct literal; the doc example never compiled and no
    // variant had used it until one did.
    ( @setup [ $( $setup:tt )* ] setup { $( $s:tt )* } $( $rest:tt )* ) => {
        $crate::__bench_expand_body!( @setup [ $( $setup )* $( $s )* ] $( $rest )* )
    };

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

/// Like [`timed!`], but auto-calibrates: it probes one execution of the `run`
/// block to size a repetition count (via [`calibrate_reps`]), then times that
/// many repetitions and reports the per-repetition time. So a run whose single
/// pass is below the counter quantum is measured above the counter's resolution
/// without the variant author hand-tuning an iteration constant. The `run` block
/// must be safe to execute several times (it is: the probe pass plus the timed
/// passes), which every fold-the-input interpreter loop already is.
///
/// ```ignore
/// fn variant<const N: usize>(input: &Input<N>, output: &mut Output<N>) -> FfiBenchCall {
///     mockspace_bench_core::timed_calibrated! {
///         setup { let d = decode(input); }
///         run { interpret(&d, output); }
///     }
/// }
/// ```
#[macro_export]
macro_rules! timed_calibrated {
    ( $( $tokens:tt )* ) => {
        $crate::__bench_calibrated_body!( @setup [] $( $tokens )* )
    };
}

#[macro_export]
macro_rules! __bench_calibrated_body {
    ( @setup [ $( $setup:tt )* ] setup { $( $s:tt )* } $( $rest:tt )* ) => {
        $crate::__bench_calibrated_body!( @setup [ $( $setup )* $( $s )* ] $( $rest )* )
    };

    ( @setup [ $( $setup:tt )* ] run { $( $run:tt )* } $( $teardown:tt )* ) => {{
        $( $setup )*
        // Probe one pass to size the calibration (also a real execution).
        let __p0 = $crate::counter::read_counter();
        { $( $run )* }
        let __p1 = $crate::counter::read_counter();
        let __reps = $crate::calibrate_reps(__p1 - __p0, $crate::CALIBRATION_FLOOR_TICKS);
        // Time __reps passes, report the per-pass tick span.
        let __start = $crate::counter::read_counter();
        let mut __r: u64 = 0;
        while __r < __reps {
            { $( $run )* }
            __r += 1;
        }
        let __end = $crate::counter::read_counter();
        $( $teardown )*
        $crate::FfiBenchCall { run_ticks: (__end - __start) / __reps }
    }};

    ( @setup [ $( $setup:tt )* ] $next:tt $( $rest:tt )* ) => {
        $crate::__bench_calibrated_body!( @setup [ $( $setup )* $next ] $( $rest )* )
    };
}

#[cfg(test)]
mod calibration_tests {
    use super::{calibrate_reps, CALIBRATION_FLOOR_TICKS};

    #[test]
    fn above_floor_needs_one_rep() {
        assert_eq!(calibrate_reps(5000, 2048), 1);
        assert_eq!(calibrate_reps(2048, 2048), 1, "exactly at floor");
    }

    #[test]
    fn below_floor_scales_to_exceed_it() {
        // 100-tick pass, 2048 floor -> ceil(2048/100) = 21 reps; 21*100 >= 2048.
        let reps = calibrate_reps(100, 2048);
        assert_eq!(reps, 21);
        assert!(reps * 100 >= 2048);
        // 1-tick pass -> 2048 reps.
        assert_eq!(calibrate_reps(1, 2048), 2048);
    }

    #[test]
    fn zero_probe_is_bounded() {
        // A zero probe is treated as one tick, capped at 2^20 for the default floor.
        assert_eq!(calibrate_reps(0, CALIBRATION_FLOOR_TICKS), 2048);
        // A pathological huge floor still caps the rep count.
        assert_eq!(calibrate_reps(0, u64::MAX), 1 << 20);
    }

    // Exercise the `timed_calibrated!` expansion end to end: the setup/run token
    // muncher, the probe pass, the calibrated repetition loop, and the per-pass
    // division. Compiling it at all proves setup tokens are hoisted before the
    // probe (the run block reads `acc`, declared in setup); running it proves the
    // division never divides by zero (calibrate_reps floors reps at 1).
    #[test]
    fn timed_calibrated_expands_and_runs() {
        let call = crate::timed_calibrated! {
            setup { let mut acc: u64 = 0; }
            run {
                acc = acc.wrapping_add(core::hint::black_box(3));
                core::hint::black_box(acc);
            }
        };
        // run_ticks is a per-pass tick span (may be 0 on a very fast pass, but is
        // a well-formed u64 produced without panicking).
        let _ticks: u64 = call.run_ticks;
    }
}
