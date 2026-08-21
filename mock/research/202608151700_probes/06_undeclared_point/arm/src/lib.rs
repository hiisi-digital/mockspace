use mockspace_bench_core::{timed, FfiBenchCall};
use mockspace_bench_macro::bench_variant;

/// Declares exactly one point. The bench.toml that names this arm may
/// perfectly well list others; nothing connects the two.
#[bench_variant("only64", sizes = [64])]
fn run<const N: usize>(input: &[u8; N], output: &mut u64) -> FfiBenchCall {
    timed! { run { let mut a = 0u64; for &b in input { a = a.wrapping_mul(31).wrapping_add(b as u64); } *output = a; } }
}
