// When the `perf-counters` feature is on (macOS), the harness links the private
// `kperf.framework`, which lives in the private-frameworks directory that is not
// on the default framework search path. Add it so `#[link(name = "kperf", kind =
// "framework")]` resolves at link time. No-op when the feature is off or off-mac.
fn main() {
    let perf = std::env::var("CARGO_FEATURE_PERF_COUNTERS").is_ok();
    let mac = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    if perf && mac {
        println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
    }
}
