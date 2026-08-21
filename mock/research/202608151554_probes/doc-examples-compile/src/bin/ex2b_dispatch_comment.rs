//! The claim the example's own trailing comment makes, at
//! `bench-core/src/lib.rs:379`:
//!     // dispatch(n, may_differ) -> Option<RoutineBridge>
//! This bin exists to test that comment, and it is NOT verbatim code.
use mockspace_bench_core::byte_routine_dispatch;
fn main() {
    let dispatch = byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 16384]);
    let _: Option<mockspace_bench_core::RoutineBridge> = dispatch(64, false);
}
