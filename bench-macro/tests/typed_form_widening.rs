//! The typed form's output size, where the output depends on `N` and is not
//! the input's size.
//!
//! `typed_form.rs` copies `[u8; N]` into `[u8; N]`, so a size export measuring
//! the input type would pass there. Here the output is eight times the input
//! at every size, so only the output type answers right.

use mockspace_bench_core::{FfiBenchCall, timed};
use mockspace_bench_macro::bench_variant;

#[bench_variant("widen-typed", sizes = [1, 3, 8])]
fn widen<const N: usize>(input: &[u8; N], output: &mut [u64; N]) -> FfiBenchCall {
    timed! {
        run {
            for (o, i) in output.iter_mut().zip(input) {
                *o = u64::from(*i);
            }
        }
    }
}

#[test]
fn the_output_size_is_the_output_types_at_each_size() {
    assert_eq!(bench_output_size(1), 8);
    assert_eq!(bench_output_size(3), 24);
    assert_eq!(bench_output_size(8), 64);
}

#[test]
fn the_output_size_is_not_the_input_size() {
    for n in [1, 3, 8] {
        assert_ne!(bench_output_size(n), n, "n={n} answered the input's size");
    }
}

#[test]
fn an_undeclared_size_has_no_output_size() {
    for n in [0, 2, 4, 7, 9, 64, usize::MAX] {
        assert_eq!(bench_output_size(n), 0, "n={n} is not in sizes = [1, 3, 8]");
    }
}

#[test]
fn the_entry_writes_exactly_the_size_it_declares() {
    // One byte past the declared 24 must survive the call. Aligned for the
    // `u64`s the entry writes through.
    #[repr(C, align(8))]
    struct Aligned([u8; 32]);
    let input = [1u8, 2, 3];
    let mut aligned = Aligned([0xAAu8; 32]);
    let buffer = &mut aligned.0;
    unsafe {
        let _ = bench_entry(input.as_ptr(), buffer.as_mut_ptr(), 3);
    }
    let mut expected = [0u8; 24];
    for (k, v) in input.iter().enumerate() {
        expected[k * 8 .. k * 8 + 8].copy_from_slice(&u64::from(*v).to_ne_bytes());
    }
    assert_eq!(&buffer[.. 24], &expected);
    assert_eq!(buffer[24], 0xAA);
}
