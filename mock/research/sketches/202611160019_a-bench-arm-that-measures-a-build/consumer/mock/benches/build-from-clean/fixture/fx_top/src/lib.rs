//! The top of the fixture's diamond.

pub fn both(xs: &[u64]) -> u64 {
    fx_left::left(xs) ^ fx_right::right(xs)
}
