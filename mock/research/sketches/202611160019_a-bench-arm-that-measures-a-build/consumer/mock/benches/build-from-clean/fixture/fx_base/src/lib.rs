//! The bottom of the fixture's diamond.

pub fn mix(x: u64) -> u64 {
    let x = x ^ (x >> 33);
    let x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^ (x >> 33)
}
