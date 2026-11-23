//! `git status` in the clone of the fsmonitor world for the arm `none_dev` (`fsmonitor_world::Arm::NoneDev`).
//! The world is made once per process, in the untimed setup.

use fsmonitor_world::{Arm, status, clone};
use mockspace_bench_core::{FfiBenchCall, abi_hash, timed};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(_input_ptr: *const u8, output_ptr: *mut u8, _n: usize) -> FfiBenchCall {
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        setup { let c = clone(Arm::NoneDev); }
        run { *output = u64::from(status(c)); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"none_dev\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}

/// One `u64`: 1 where git succeeded.
#[unsafe(no_mangle)]
pub extern "C" fn bench_output_size(_n: usize) -> usize {
    core::mem::size_of::<u64>()
}
