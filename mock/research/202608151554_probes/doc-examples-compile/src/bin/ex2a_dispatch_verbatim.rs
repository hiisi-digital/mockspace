//! `bench-core/src/lib.rs:377-380`, the `byte_routine_dispatch!` example,
//! the code line only.
use mockspace_bench_core::byte_routine_dispatch;
fn main() {
    // --- BEGIN verbatim ---
    let dispatch = byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 16384]);
    // --- END verbatim ---
    let _ = dispatch;
}
