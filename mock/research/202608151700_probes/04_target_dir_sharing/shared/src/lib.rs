#[inline(never)]
pub fn common(x: u64) -> u64 {
    if cfg!(feature = "fast") { x ^ 0xF457 } else { x.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17) }
}
/// Compiled-in witness of which feature set this rlib was built with.
#[unsafe(no_mangle)]
pub extern "C" fn shared_feature_witness() -> u64 { if cfg!(feature = "fast") { 1 } else { 0 } }
