//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

// `probe.rs` with one more exported function placed before the rest, linked
// as `variant_shifted`: the same `bench_entry` and `do_work` at other
// addresses. See README.md.

#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_align(x: u64, y: u64) -> u64 {
    let mut acc = x;
    for i in 0 .. (y & 15) {
        acc = acc.rotate_left(17).wrapping_mul(0x9e3779b97f4a7c15) ^ i;
    }
    acc
}

#[inline(never)]
fn do_work(input: &[u8; 8], output: &mut u64) {
    let mut acc: u64 = 0;
    for &b in input.iter() {
        acc = acc.wrapping_add(b as u64).wrapping_mul(31);
    }
    *output = acc;
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    concat!(module_path!(), "\0").as_ptr()
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
        // The name's address is layout: RIP-relative on x86_64, `adrp`
        // and `add` on aarch64.
        _ => bench_name() as u64,
    }
}
