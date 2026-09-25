//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Integration tests for the routine form of `#[bench_variant]`, where the
//! input and output types come from `<Algo<N> as Routine>`.
//!
//! The output here is a different size at every `N` on purpose, so the size
//! export has to come from the routine at that `N` and cannot pass by being
//! right at one of them.

use mockspace_bench_core::{FfiBenchCall, Routine, timed};
use mockspace_bench_macro::bench_variant;

pub struct Widen<const N: usize>;

impl<const N: usize> Routine for Widen<N> {
    type Input = u8;
    type Output = [u8; N];

    fn build_input(seed: u64) -> u8 {
        seed as u8
    }
}

#[bench_variant(Widen, "widen", sizes = [1, 24, 40])]
fn widen<const N: usize>(
    input: &<Widen<N> as Routine>::Input,
    output: &mut <Widen<N> as Routine>::Output,
) -> FfiBenchCall {
    timed! {
        run { *output = [*input; N]; }
    }
}

#[test]
fn the_output_size_is_the_routines_at_each_declared_size() {
    assert_eq!(bench_output_size(1), <Widen<1> as Routine>::output_size());
    assert_eq!(bench_output_size(24), <Widen<24> as Routine>::output_size());
    assert_eq!(bench_output_size(40), <Widen<40> as Routine>::output_size());
    assert_eq!(bench_output_size(40), 40);
}

#[test]
fn an_undeclared_size_has_no_output_size() {
    for n in [0, 2, 23, 25, 48, usize::MAX] {
        assert_eq!(bench_output_size(n), 0, "n={n} is not in sizes = [1, 24, 40]");
    }
}

#[test]
fn the_entry_writes_exactly_the_size_it_declares() {
    // A buffer one byte longer than declared, whose last byte must survive the
    // call: the entry writes the declared size and not a byte past it.
    let input = 7u8;
    let mut buffer = [0xAAu8; 25];
    unsafe {
        let _ = bench_entry(&input as *const u8, buffer.as_mut_ptr(), 24);
    }
    assert_eq!(&buffer[.. 24], &[7u8; 24]);
    assert_eq!(buffer[24], 0xAA);
}
