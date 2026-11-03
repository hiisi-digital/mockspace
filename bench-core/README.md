# `mockspace-bench-core`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> The routine trait, the ffi types and the counter that variant benchmarks for mockspace are written against. `no_std` by default.

</div>

A benchmark that compares implementations of one algorithm needs the same input, the same validation and the same way of reading the clock for every one of them, or the comparison measures the harness more than the algorithms. This crate holds those shared parts. One `Routine` implementation defines what is computed (input shape, output shape, validation, scoring, ops count), and any number of variant implementations of it each compile to a self-contained dylib that `mockspace-bench-harness` loads in isolation.

Timing is by hardware counter, `CNTVCT_EL0` on aarch64 and `rdtsc` on x86_64, through the `counter` module, and the `timed!` macro wraps a setup, run and teardown block with it. The crate is `no_std` by default, which is enough for a variant to time itself in an `extern "C"` entry point, with the harness side adding `std`.

## Usage

The crate is not on crates.io, so a consumer takes it from git:

```toml
[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }
```

A `Routine` is defined once per algorithm, with default methods for validation, scoring, the ops count and input tagging:

```rust
use mockspace_bench_core::Routine;

pub struct ContentHash;

impl Routine for ContentHash {
    type Input = [u8; 64];
    type Output = u64;

    fn build_input(seed: u64) -> Self::Input {
        let mut bytes = [0u8; 64];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (seed.wrapping_mul(0x100000001b3) ^ i as u64) as u8;
        }
        bytes
    }

    fn ops_per_call(_input: &Self::Input) -> u64 {
        64 // bytes hashed per call
    }
}
```

Each implementation is then one variant, its own cdylib, written with `#[bench_variant]` from `mockspace-bench-macro`:

```rust
use mockspace_bench_core::{FfiBenchCall, timed};
use mockspace_bench_macro::bench_variant;

#[bench_variant("fnv", sizes = [64])]
fn fnv<const N: usize>(input: &[u8; N], output: &mut u64) -> FfiBenchCall {
    timed! {
        run { *output = my_hash_impl(input); }
    }
}
```

The macro writes the four exports the harness loads, `bench_entry`, `bench_name`, `bench_abi_hash` and `bench_output_size`, and a variant written by hand exports the same four. `bench_output_size(n)` answers the bytes `bench_entry` writes at `n`, 0 where it has no such size. The harness loads each variant in its own process, refuses one built against another `bench-core` or writing another size than the routine's output, runs the rest on identical inputs, validates, scores, and writes a CSV and the findings report.

## Extras

### Status

Version 1 of the surface: the `Routine` trait, the `FfiBenchCall`, `BenchEntryFn`, `BenchNameFn` and `AbiHashFn` types, `RoutineBridge` with the `routine_bridge!` macro for harness-side dispatch (std only), the `counter` module with `read_counter()`, `counter_frequency()`, `ticks_to_ns()`, `ticks_to_us()`, `pin_to_perf_cores()` and `evict_cache_range()`, the `timed!` macro, and `abi_hash()` as a `const fn` for checking the ABI version on dylib load.

### Cargo features

| Feature | Default | Effect |
|---|---|---|
| `std` | off | Enables `RoutineBridge`, `routine_bridge!`, the time conversions and P-core pinning on macOS. |

## Support

Feel free to contribute! If unsure about wasting work, the best practice is to throw in an issue describing what you'd do, and only then commit to writing a big PR, because chances are, it might not be something that belongs here. However, forks are always a valid choice and we'd encourage everyone to experiment and have their own takes on this. When doing this, do mind the license(s) though!

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/dev/LICENSE)
