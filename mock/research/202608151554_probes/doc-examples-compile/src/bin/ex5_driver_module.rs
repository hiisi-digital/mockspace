//! `bench-harness/src/driver/mod.rs:11-19`, the consumer `main` example.
//! Scaffolding the example names but does not show: `build_workload`,
//! `routine_for`, and the two imports its body uses unqualified.
use mockspace_bench_core::byte_routine_dispatch;
use mockspace_bench_harness::driver::DriverRegistry;
use mockspace_bench_harness::{BenchConfig, RoutineSpec, Workload};

fn build_workload(_name: &str, _n: usize) -> Workload { Workload::new() }
fn routine_for(_c: &BenchConfig) -> Option<RoutineSpec> { None }

// --- BEGIN verbatim ---
fn main() -> std::process::ExitCode {
    mockspace_bench_harness::driver::drive(&DriverRegistry {
        build_workload,
        routine_for,
        byte_dispatch: byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 16384]),
    })
}
// --- END verbatim ---
