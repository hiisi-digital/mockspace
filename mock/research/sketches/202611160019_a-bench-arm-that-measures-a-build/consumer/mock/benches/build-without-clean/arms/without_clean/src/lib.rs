//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `build-without-clean` arm `without_clean`: the control. The fixture built with nothing removed first.

use mockspace_bench_core::{FfiBenchCall, abi_hash, timed};

/// One call: nothing removed, the build timed, the record taken untimed. Answers with the number of fixture crates
/// the compiler built.
///
/// # Safety
///
/// The harness passes an eight-byte output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(
    _input_ptr: *const u8,
    output_ptr: *mut u8,
    _point: usize,
) -> FfiBenchCall {
    timed! {
        setup {
            let absent_before = clean_build::absent();
        }
        run {
            let said = clean_build::build();
        }
        let compiled = clean_build::record(&said, absent_before);
        unsafe { output_ptr.cast::<u64>().write_unaligned(compiled) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"without_clean\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_output_size(_n: usize) -> usize {
    8
}
