//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------


// The source the recorded fixtures beside this file were compiled from, as
// `variant_scalar` and `variant_unrolled_x4`. See README.md.

#[inline(never)]
fn do_work(input: &[u8; 8], output: &mut u64) {
    let mut acc: u64 = 0;
    for &b in input.iter() {
        acc = acc.wrapping_add(b as u64).wrapping_mul(31);
    }
    *output = acc;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(input_ptr: *const u8, output_ptr: *mut u8, n: usize) -> u64 {
    match n {
        8 => {
            let input = unsafe { &*(input_ptr as *const [u8; 8]) };
            let output = unsafe { &mut *(output_ptr as *mut u64) };
            do_work(input, output);
            0
        },
        _ => 0,
    }
}
