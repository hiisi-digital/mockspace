//! The typed form's output size, where the output does not depend on `N`.
//!
//! The size arm shadows the const generic before measuring, which matters only
//! when the output names `N`; here it does not, so this pins the other path:
//! one size at every `N`, smaller than any input.

use mockspace_bench_core::{FfiBenchCall, timed};
use mockspace_bench_macro::bench_variant;

#[bench_variant("count-typed", sizes = [8, 16, 64])]
fn count<const N: usize>(input: &[u8; N], output: &mut u32) -> FfiBenchCall {
    timed! {
        run { *output = input.iter().filter(|b| **b != 0).count() as u32; }
    }
}

#[test]
fn the_output_size_is_the_same_at_every_declared_size() {
    for n in [8, 16, 64] {
        assert_eq!(bench_output_size(n), core::mem::size_of::<u32>(), "n={n}");
    }
}

#[test]
fn the_output_size_is_not_the_input_size() {
    for n in [8, 16, 64] {
        assert_ne!(bench_output_size(n), n, "n={n} answered the input's size");
    }
}

#[test]
fn an_undeclared_size_has_no_output_size() {
    for n in [0, 4, 32, 128, usize::MAX] {
        assert_eq!(bench_output_size(n), 0, "n={n} is not in sizes = [8, 16, 64]");
    }
}

#[test]
fn the_entry_writes_exactly_the_size_it_declares() {
    let mut input = [0u8; 16];
    input[3] = 1;
    input[9] = 7;
    // Aligned for the `u32` the entry writes through.
    #[repr(C, align(4))]
    struct Aligned([u8; 8]);
    let mut aligned = Aligned([0xAAu8; 8]);
    let buffer = &mut aligned.0;
    unsafe {
        let _ = bench_entry(input.as_ptr(), buffer.as_mut_ptr(), 16);
    }
    assert_eq!(&buffer[.. 4], &2u32.to_ne_bytes());
    assert_eq!(buffer[4], 0xAA);
}
