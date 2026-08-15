//! Probe: an arm's declared point set is not askable, so a manifest point the
//! arm does not implement is discovered by calling it.
//!
//! The arm ABI is exactly three symbols (`bench_entry`, `bench_name`,
//! `bench_abi_hash`; see `bench-harness/src/harness.rs:122-138`). None of
//! them reports which `n` the arm was compiled for, so the harness cannot
//! check a manifest point against an arm ahead of time.
//!
//! NEGATIVE CONTROL: the DECLARED point must succeed. If n=64 also fails,
//! the probe is measuring a broken build rather than an undeclared point.
use mockspace_bench_core::FfiBenchCall;

type Entry = unsafe extern "C" fn(*const u8, *mut u8, usize) -> FfiBenchCall;

fn call(lib: &libloading::Library, n: usize) {
    let input = vec![7u8; n.max(1)];
    let mut out = 0u64;
    unsafe {
        let e: libloading::Symbol<Entry> = lib.get(b"bench_entry").unwrap();
        let r = e(input.as_ptr(), (&raw mut out).cast(), n);
        println!("  n={n}: returned, run_ticks={} out={out}", r.run_ticks);
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("dylib path");
    let which = std::env::args().nth(2).expect("64 or 128");
    let lib = unsafe { libloading::Library::new(&path).unwrap() };
    println!("symbols an arm exports that name its point set: none");
    println!("  bench_entry / bench_name / bench_abi_hash is the whole ABI");
    call(&lib, which.parse().unwrap());
}
