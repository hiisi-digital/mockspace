#[cfg(feature = "fast")]
pub fn common(x: u64) -> u64 { x.wrapping_mul(3) }
#[cfg(not(feature = "fast"))]
pub fn common(x: u64) -> u64 { x.wrapping_add(3) }
