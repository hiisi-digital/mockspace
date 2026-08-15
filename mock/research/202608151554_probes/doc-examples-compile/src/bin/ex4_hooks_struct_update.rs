//! `bench-harness/src/driver/hooks.rs:42-44`, the struct-update example.
//! Scaffolding the example names but does not show: `my_table`, which the
//! surrounding doc says is built with `routine_table!`.
use mockspace_bench_harness::driver::Hooks;
use mockspace_bench_harness::{routine_table, BenchConfig, RoutineSpec};

struct Keyed<const K: usize>;
impl<const K: usize> mockspace_bench_core::Routine for Keyed<K> {
    type Input = u64;
    type Output = u64;
    fn build_input(seed: u64) -> u64 { seed ^ K as u64 }
}

fn main() {
    let my_table: fn(&BenchConfig) -> Option<RoutineSpec> =
        routine_table! { "warm" => Keyed[64, 256] };
    // --- BEGIN verbatim ---
    let hooks = Hooks { routine_for: Some(my_table), ..Hooks::default() };
    // --- END verbatim ---
    let _ = hooks;
}
