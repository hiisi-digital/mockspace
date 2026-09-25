# mockspace-bench-core

Canonical bench framework primitives for mockspace consumers.

## What it is

A small `no_std`-by-default crate that exposes a `Routine` trait and an
FFI bridge so consumers can write variant-comparison benchmarks. One
`Routine` impl defines what is computed (input shape, output shape,
validation, scoring, ops count); multiple variant impls per Routine
each compile to a self-contained dylib that the harness loads in
isolation. Hardware counter timing (`CNTVCT_EL0` on aarch64, `rdtsc`
on x86_64) is provided by the `counter` module; the `timed!` macro
gives a simple setup/run/teardown timing block.

The framework was extracted from polka-dots' `mock/benches/bench-core/`
and lifted into mockspace so every consumer gets the canonical surface.

## Status

v1. The framework crate ships:

- `Routine` trait with default-method validation, scoring, ops count, input tagging.
- `FfiBenchCall`, `BenchEntryFn`, `BenchNameFn`, `AbiHashFn` FFI types.
- `RoutineBridge` and `routine_bridge!` macro for harness-side dynamic dispatch (std-only).
- `counter` module with `read_counter()`, `counter_frequency()`, `ticks_to_ns()`, `ticks_to_us()`, `pin_to_perf_cores()`, `evict_cache_range()`.
- `timed!` macro with setup/run/teardown phases.
- `abi_hash()` const fn for ABI version checking on dylib load.

The harness orchestrator (variant isolation, CSV cache, multi-process
timing, findings generation) lives in polka-dots as `mock-bench` today.
Lifting it into mockspace as `mockspace-bench-harness` is v2 scope.

## Cargo features

| Feature | Default | Effect |
|---|---|---|
| `std` | off | Enables `RoutineBridge`, `routine_bridge!`, time conversions, P-core pinning on macOS. |

`no_std` mode supplies the trait surface, FFI types, the counter,
`abi_hash()`, and `timed!`. That is enough for variant impls to time
themselves in `extern "C"` entry points; the harness adds `std` later.

## Installation

Until mockspace ships a stable release, consume via git:

```toml
[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }
```

## Usage

Define a Routine once per algorithm:

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

Then write one variant per implementation, each its own cdylib, with
`#[bench_variant]` from `mockspace-bench-macro`:

```rust
use mockspace_bench_core::{FfiBenchCall, timed};
use mockspace_bench_macro::bench_variant;

#[bench_variant("fnv", sizes = [64])]
fn fnv<const N: usize>(input: &[u8; N], output: &mut u64) -> FfiBenchCall {
    timed! {
        run { *output = your_hash_impl(input); }
    }
}
```

The macro writes the four exports the harness loads: `bench_entry`,
`bench_name`, `bench_abi_hash` and `bench_output_size`. A variant
written by hand exports the same four, and `bench_output_size(n)`
answers the bytes `bench_entry` writes at `n`, 0 where it has no such
size. `mockspace-bench-harness` loads each variant in its own process,
refuses one built against another `bench-core` or writing another
size than the routine's output, runs the rest on identical inputs,
validates, scores, and writes CSV plus the findings report.

## Origin

Framework code originated in
[polka-dots](https://github.com/orgrinrt/polka-dots)'
`mock/benches/bench-core/`. Lifted into mockspace under MPL-2.0.

## License

`SPDX-License-Identifier: MPL-2.0`
