//! Integration tests for the typed form of `#[bench_variant]`.
//!
//! Exercises the macro path that does NOT require `Algo<N>: Routine`
//! in the variant crate. The macro reads input/output types from the
//! function signature directly.

use mockspace_bench_core::{timed, FfiBenchCall};
use mockspace_bench_macro::bench_variant;

#[bench_variant("identity-typed", sizes = [8, 16, 32])]
fn run_identity<const N: usize>(input: &[u8; N], output: &mut [u8; N]) -> FfiBenchCall {
    timed! {
        run { *output = *input; }
    }
}

#[test]
fn macro_generates_named_export() {
    unsafe {
        let ptr = bench_name();
        assert!(!ptr.is_null());
        // bench_name returns a NUL-terminated cstring.
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let bytes = core::slice::from_raw_parts(ptr, len);
        assert_eq!(bytes, b"identity-typed");
    }
}

#[test]
fn macro_generates_abi_hash_export() {
    let h = bench_abi_hash();
    assert_ne!(h, 0, "abi_hash should be non-zero");
}

#[test]
fn dispatch_at_supported_size_copies_input_to_output() {
    let input: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut output: [u8; 8] = [0; 8];
    unsafe {
        let _ = bench_entry(input.as_ptr(), output.as_mut_ptr(), 8);
    }
    assert_eq!(input, output);
}

#[test]
fn dispatch_at_other_supported_size() {
    let input: [u8; 16] = [42; 16];
    let mut output: [u8; 16] = [0; 16];
    unsafe {
        let _ = bench_entry(input.as_ptr(), output.as_mut_ptr(), 16);
    }
    assert_eq!(input, output);
}

// Note: a "panic on unsupported size" test is intentionally NOT
// included. The macro emits `panic!` inside an extern "C" function,
// which aborts (cannot unwind across the FFI boundary) and would
// SIGABRT the test process. The harness handles this at runtime by
// catching the worker subprocess exit; in a test binary, the panic
// remains visible via stderr but the process aborts.
