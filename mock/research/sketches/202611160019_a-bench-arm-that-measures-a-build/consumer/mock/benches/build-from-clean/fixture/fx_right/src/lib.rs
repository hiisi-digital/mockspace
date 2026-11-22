//! The other side of the fixture's diamond.

pub fn right(xs: &[u64]) -> u64 {
    xs.iter().rev().fold(1, |acc, &x| acc ^ fx_base::mix(x.rotate_left(7)))
}
