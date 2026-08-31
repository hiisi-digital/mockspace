//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Multiply-xor mixer. SplitMix64 finalizer.

use mockspace_bench_core::{FfiBenchCall, abi_hash, timed};

#[inline(always)]
fn mix(input: u64) -> u64 {
    let mut x = input;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { *output = mix(*input); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"multiply-xor\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
