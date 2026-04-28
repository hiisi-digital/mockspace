//! Rotate-based mixer. Uses rotate-left rather than logical right
//! shifts in the avalanche steps, exercising the cpu's ROT
//! instruction. Functionally distinct from the multiply-xor variant.

use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

#[inline(always)]
fn mix(input: u64) -> u64 {
    let mut x = input;
    x ^= x.rotate_left(31);
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x.rotate_left(17);
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^ x.rotate_left(13)
}

#[no_mangle]
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

#[no_mangle]
pub extern "C" fn bench_name() -> *const u8 {
    b"bitwise-rotate\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
