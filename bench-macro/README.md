# `mockspace-bench-macro`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> The attribute macro that turns one generic function into a bench variant a harness can load. Proc macro, unpublished.

</div>

`#[bench_variant("name", sizes = [64, 128, 256])]` is the typed form, and `#[bench_variant(Algo, "name", sizes = [64, 128, 256])]` the routine form for a variant that needs the `<Algo<N> as Routine>` projection, which requires `Algo<N>: Routine`. Either decorates a generic function and emits the `extern "C"` exports the harness loads from each variant cdylib (`bench_entry`, `bench_name`, `bench_abi_hash` and `bench_output_size`), along with a dispatch table over the `sizes` list. The sizes are an explicit attribute argument, so a consumer needs no sidecar file naming them.

`bench_output_size(n)` answers the size of the output the entry writes at `n`, and 0 for a size not in the list, and the harness refuses a variant whose answer differs from the output buffer of the routine it resolved, before calling it.

## Usage

The crate is not on crates.io, so a consumer takes it from git beside `mockspace-bench-core`:

```toml
[dependencies]
mockspace-bench-macro = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }
```

The routine form, for a variant over a sparse matrix routine:

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

The function's own `where` clauses are propagated unchanged, and it takes exactly one const generic parameter, which is the dispatched size.

## Support

Feel free to contribute! If unsure about wasting work, the best practice is to throw in an issue describing what you'd do, and only then commit to writing a big PR, because chances are, it might not be something that belongs here. However, forks are always a valid choice and we'd encourage everyone to experiment and have their own takes on this. When doing this, do mind the license(s) though!

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/dev/LICENSE)
