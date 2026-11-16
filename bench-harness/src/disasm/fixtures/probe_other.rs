//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

// The source the recorded fixtures beside this file were linked from, as
// `variant_other`: `probe.rs` with different work. See README.md. `no_std` so a
// linker for every target exists on one host; `bench_name` puts the crate's
// name in the library, as a real arm's does, so the two libraries lay out
// differently around identical code.

#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[inline(never)]
fn do_work(input: &[u8; 8], output: &mut u64) {
    let mut acc: u64 = 0;
    for &b in input.iter() {
        acc = (acc ^ b as u64).wrapping_mul(0x100000001b3);
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
