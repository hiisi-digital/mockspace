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
(the substrate that drove arvo's strategy-marker design) and lifted
into mockspace so every consumer gets the canonical surface.

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
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", features = ["std"] }
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

Then write one variant per implementation:

```rust
use mockspace_bench_core::{FfiBenchCall, timed};

#[no_mangle]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = &*(input_ptr as *const [u8; 64]);
    let output = &mut *(output_ptr as *mut u64);
    timed! {
        run { *output = your_hash_impl(input); }
    }
}
```

The harness (v2) loads each variant dylib, runs them on identical
inputs, validates, scores, and writes CSV plus `findings.md`. Until v2,
consumers can write variants and run them via `cargo bench` or a
hand-rolled harness; the Routine surface is forward-compatible.

## Origin

Framework code originated in
[polka-dots](https://github.com/orgrinrt/polka-dots)'
`mock/benches/bench-core/`. Lifted into mockspace under MPL-2.0.

## License

`SPDX-License-Identifier: MPL-2.0`
