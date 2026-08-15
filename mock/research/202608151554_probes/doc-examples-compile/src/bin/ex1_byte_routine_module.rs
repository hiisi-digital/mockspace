//! `bench-core/src/byte_routine.rs:11-18`, the "Orchestrator side" example.
//! Transcribed verbatim between the BEGIN/END markers. No scaffolding is
//! needed: the example declares its own imports.
fn main() {
    // --- BEGIN verbatim ---
    use mockspace_bench_core::{routine_bridge, ByteRoutine, RoutineSpec};

    // FNV1a vs xxHash3 over 64-byte inputs, 8-byte digests, algos
    // produce different digests for the same input.
    type HashRoutine64 = ByteRoutine<64, 8, true>;
    let bridge = routine_bridge!(HashRoutine64);
    // --- END verbatim ---
    let _ = bridge;
}
