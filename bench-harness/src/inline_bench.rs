//! Inlined benchmark mode.
//!
//! Compiles variant code directly into the harness binary (rlib, not
//! cdylib), calling through generic trait methods. LLVM can see
//! through the dispatch mechanism, enabling devirtualisation and
//! inlining: precisely what the cdylib boundary prevents.
//!
//! Use case: measuring dispatch strategy overhead (WU dispatch,
//! function pointer tables) where the cdylib boundary would mask
//! LLVM's ability to optimise through the dispatch.
//!
//! Define inline variants as functions with the same signature as
//! `bench_entry`, register them with the inline harness via
//! [`InlineVariant`], and call [`run_inline`].

use crate::core::counter;
use crate::core::FfiBenchCall;

/// An inlined variant: function pointer + name, compiled into the
/// harness binary. No dylib boundary. LLVM can inline.
pub struct InlineVariant {
    pub name: &'static str,
    pub entry: fn(*const u8, *mut u8, usize) -> FfiBenchCall,
}

/// Run a set of inlined variants for comparison. `black_box` is
/// applied to inputs to prevent constant folding.
pub fn run_inline(
    variants: &[InlineVariant],
    input_builder: fn(u64) -> Vec<u8>,
    output_size: usize,
    n: usize,
    iterations: usize,
    seed: u64,
) {
    let input = input_builder(seed);
    let mut output = vec![0u8; output_size];

    eprintln!(
        "  Inline benchmark: {} variants × {} iterations",
        variants.len(),
        iterations
    );

    for v in variants {
        for _ in 0..iterations / 10 {
            let inp = std::hint::black_box(input.as_ptr());
            let out = std::hint::black_box(output.as_mut_ptr());
            (v.entry)(inp, out, n);
        }

        let start = counter::read_counter();
        for _ in 0..iterations {
            let inp = std::hint::black_box(input.as_ptr());
            let out = std::hint::black_box(output.as_mut_ptr());
            std::hint::black_box((v.entry)(inp, out, n));
        }
        let end = counter::read_counter();

        let total_ns = counter::ticks_to_ns(end - start);
        let per_call_ns = total_ns / iterations as f64;
        eprintln!(
            "  {:24} {:8.1} ns/call  ({:.1} µs total)",
            v.name,
            per_call_ns,
            total_ns / 1000.0
        );
    }
}
