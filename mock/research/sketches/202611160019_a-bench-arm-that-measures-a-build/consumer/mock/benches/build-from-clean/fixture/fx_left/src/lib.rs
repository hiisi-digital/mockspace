//! One side of the fixture's diamond.

pub fn left(xs: &[u64]) -> u64 {
    xs.iter().fold(0, |acc, &x| acc.wrapping_add(fx_base::mix(x)))
}
