# mockspace-bench-macro

Proc macro for the mockspace bench harness.

## What it is

`#[bench_variant(Algo, "name", sizes = [64, 128, 256])]` decorates a
generic function and emits the `extern "C"` exports the harness loads
from each variant cdylib (`bench_entry`, `bench_name`,
`bench_abi_hash`) plus an N-dispatch table built from the `sizes`
list.

Generalised port of `polka-dots/mock/benches/bench-macro/`. The
polka-dots version hardcoded `include!("../../supported_n.rs")` to
pull a const slice of supported sizes; mockspace lifts that to an
explicit attribute argument so consumers do not need a sidecar file.

## Usage

```rust
use mockspace_bench_core::{FfiBenchCall, Routine, timed};
use mockspace_bench_macro::bench_variant;

#[bench_variant(SpMV, "csr-scalar", sizes = [64, 128, 256, 512, 1024])]
fn variant<const N: usize>(
    input: &<SpMV<N> as Routine>::Input,
    output: &mut <SpMV<N> as Routine>::Output,
) -> FfiBenchCall
where
    [(); N + 1]:,
{
    timed! {
        run { csr_scalar::<N>(input, output); }
    }
}
```

The function's own `where` clauses are propagated unchanged. The
attribute requires exactly one const generic parameter (the dispatched
size).

## License

`SPDX-License-Identifier: MPL-2.0`
