//! A small support crate of the shape a bench tree carries under `support/`: a library with
//! its own tests, no dependencies, nothing that needs a harness to run.

/// The median of a slice, or `None` when it is empty. An even count takes the lower middle,
/// so the result is always one of the inputs.
pub fn median(values: &[u64]) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len().checked_sub(1)? / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_nothing_is_nothing() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(median(&[9, 1, 5]), Some(5));
    }

    #[test]
    fn the_median_of_an_even_count_is_the_lower_middle() {
        assert_eq!(median(&[4, 1, 3, 2]), Some(2));
    }
}
