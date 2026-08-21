//! `bench-harness/src/spec.rs:24-31`, the `RoutineSpec` construction example.
//! Scaffolding the example names but does not show: a `ContentHash` Routine.
struct ContentHash;
impl mockspace_bench_core::Routine for ContentHash {
    type Input = u64;
    type Output = u64;
    fn build_input(seed: u64) -> u64 { seed }
}
fn main() {
    // --- BEGIN verbatim ---
    use mockspace_bench_core::routine_bridge;
    use mockspace_bench_harness::RoutineSpec;

    let spec = RoutineSpec {
        name: "ContentHash".into(),
        bridge: routine_bridge!(ContentHash),
    };
    // --- END verbatim ---
    let _ = spec;
}
