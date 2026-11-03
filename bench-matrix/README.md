# `mockspace-bench-matrix`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> A benchmark matrix of isolated cells swept across parameter axes, with the measurement discipline built in. Unpublished, on top of the mockspace bench harness.

</div>

A benchmark built from many cells, each timed on its own, swept across parameter axes, is easy to get subtly wrong, since the scaffolding around a cell decides what the number means as much as the cell does. This crate is that scaffolding, written once so every consumer gets the same measurement discipline and only authors its domain: the program generator, the value semantics and the cell functions themselves.

A family is declared (say, dispatch shape), with a set of cells to compare within it (switch, fntable, threaded and so on), a sweep axis that produces one bench per value (six program profiles, for instance) and a size sweep. Each `(sweep value, cell, size)` becomes its own release cdylib, built under fat LTO with one codegen unit and run in its own subprocess, so measuring one cell cannot contaminate another. The harness supplies that transport, and this crate supplies the part that makes each individual measurement honest and each cell ergonomic to write.

A cell is a real, type-checked `pub fn` in the consumer crate, and each generated variant is a one-line call to it by path. The sibling crate that gets generated never contains a cell body, only the path, which means the generator is a plain function over data with no proc-macro and no closure-token capture. Per-variant LTO inlines the cell into the isolated cdylib the same way it would be inlined in place, so the measured machine code is the same as with a body spliced in as a string, while the authoring gets type checking, IDE support and borrow-check feedback.

## Usage

The crate is not on crates.io, so a consumer takes it from git, beside `mockspace-bench-core` and `mockspace-bench-harness`:

```toml
[dependencies]
mockspace-bench-matrix = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }
```

The `bench_matrix!` macro emits the cell functions and the `matrix_decls()` data from one declarative block. It goes in a `pub mod bench` of the consumer library crate, since the cells have to be `pub` for the sibling variant crates to name them by path, and a four-line generator binary calls the result.

The hand-written equivalent is plain code, and the macro is plain `macro_rules!` that expands to exactly that: `pub fn setup(..)`, `pub fn cell_<tag>(..)` and `pub fn matrix_decls() -> Vec<MatrixDecl>`. The hand form is for a family that needs shaping the macro grammar does not cover.

## Example

```rust
// in the consumer library crate: consumer::bench
pub struct DispatchState { pub d: Decoded, pub r: Vec<u64>, pub sinks: Vec<u32> }

bench_matrix! {
    name: "carrier_dispatch",
    crate_path: bench_carrier,
    crate_dep: "bench-carrier = {{ path = \"../../carrier\"{carrier_features} }}",
    extra_deps: [ "mockspace-bench-core = { path = \"...\" }" ],
    seed: 0x5eed_d15b_a7c4_0002,
    sweep profile in ["real", "madd", "tight"],
    sizes: [64, 256, 1024, 4096, 16384],
    baseline: "switch",
    floor:    "nullfloor",
    regime:   warm,                 // warm | cold_cycle(M)

    // the shared S term: built once, timed on every call so it can never hide.
    setup |profile: &str, n: usize| -> DispatchState {
        let mut gp = GenParams::profile(profile).unwrap();
        gp.node_count = n;
        let prog = generate(&gp);
        let sinks = sinks(&prog);
        let d = Decoded::parse(&encode(&prog, &REC24), REC24).unwrap();
        let r = vec![0u64; d.node_count];
        DispatchState { d, r, sinks }
    }

    // each cell returns ONE keep-alive u64; every cell folds the SAME sink set, so
    // the fidelity fold is symmetric across cells by construction.
    cell switch    |s, seed| { interpret(&s.d, seed, &mut s.r);         checksum_at(&s.r, &s.sinks) }
    cell fntable   |s, seed| { interpret_fntable(&s.d, seed, &mut s.r); checksum_at(&s.r, &s.sinks) }
    cell nullfloor |s, seed| { interpret_null(&s.d, seed, &mut s.r);    checksum_at(&s.r, &s.sinks) }
}
```

A feature-gated cell carries `#[feature = "..."]` between the tag and the closure, and those features flow into the sibling's carrier dependency. A cell that needs a different construction declares its own `setup |..| -> St { .. }` before the closure (the `direct`-style cell), and the scaffold times that per-cell setup instead of the shared one, with the cell reading that setup's own state type. `cold_cycle(M)` cells take a third parameter `k`, the iteration index, as in `cell switch |s, k, seed|`. `baseline` and `floor` name cells by tag and are checked at expansion, so a typo is a compile error and not a silent wrong baseline.

The generator binary is all of this:

```rust
// the whole generator binary
fn main() -> std::io::Result<()> {
    let decls = bench_carrier::bench::matrix_decls();
    mockspace_bench_matrix::generate_all(&decls, std::path::Path::new("."))
}
```

`generate_all` writes one isolated variant crate per `(sweep value, cell)`, each a one-line call to `scaffold::warm` (or `scaffold::cold_cycle`) naming the setup and the cell by path, and rewrites the `bench.toml` sections. The measurement logic lives in the scaffold, so the generated files hold nothing that can be measured wrong.

The same block written by hand, which is the exact expansion of the macro:

```rust
// in the consumer library crate: consumer::bench
pub struct DispatchState { pub d: Decoded, pub r: Vec<u64>, pub sinks: Vec<u32> }

pub fn setup(profile: &str, n: usize) -> DispatchState {
    let mut gp = GenParams::profile(profile).unwrap();
    gp.node_count = n;
    let prog = generate(&gp);
    let sinks = sinks(&prog);
    let d = Decoded::parse(&encode(&prog, &REC24), REC24).unwrap();
    let r = vec![0u64; d.node_count];
    DispatchState { d, r, sinks }
}

// each cell returns ONE keep-alive u64; every cell folds the SAME sink set, so the
// fidelity fold is symmetric by construction.
pub fn cell_switch(s: &mut DispatchState, seed: u64) -> u64 {
    interpret(&s.d, seed, &mut s.r);
    checksum_at(&s.r, &s.sinks)
}
pub fn cell_fntable(s: &mut DispatchState, seed: u64) -> u64 {
    interpret_fntable(&s.d, seed, &mut s.r);
    checksum_at(&s.r, &s.sinks)
}

pub fn matrix_decls() -> Vec<mockspace_bench_matrix::MatrixDecl> {
    use mockspace_bench_matrix::{MatrixDecl, CellDecl, SweepAxis, Regime};
    let cell = |tag: &str, feats: &[&str]| CellDecl {
        tag: tag.into(),
        op_path: format!("{}::cell_{tag}", module_path!()),
        setup_path: None,
        features: feats.iter().map(|s| s.to_string()).collect(),
    };
    vec![MatrixDecl {
        name: "carrier_dispatch".into(),
        crate_path: "bench_carrier".into(),
        crate_dep: "bench-carrier = {{ path = \"../../carrier\"{carrier_features} }}".into(),
        extra_deps: vec![/* bench-core, bench-macro, bench-matrix dep lines */],
        master_seed: "0x5eed_d15b_a7c4_0002".into(),
        sweep: SweepAxis { name: "profile".into(), values: ["real","madd","tight"].iter().map(|s| s.to_string()).collect() },
        sizes: vec![64, 256, 1024, 4096, 16384],
        baseline: "switch".into(),
        floor: Some("nullfloor".into()),
        regime: Regime::Warm,
        setup_path: format!("{}::setup", module_path!()),
        cells: vec![cell("switch", &[]), cell("fntable", &[])],
    }]
}
```

## Motivation

The scaffold takes the cell as a generic `FnMut` parameter and never as a `fn` pointer. A generic parameter is monomorphized and inlined with no indirection, whereas a `fn` pointer brings back an indirect call the optimizer may decline to devirtualize, and the measured region would then include a call the real deployment would not. That one signature choice carries the whole isolation argument, and `scaffold::warm` and `scaffold::cold_cycle` enforce it.

The disciplines the scaffold owns, so that a consumer never touches them:

- **Anti-hoist chain.** `scaffold` seeds `acc` from `output[0]`, folds one keep-alive per iteration and writes it back inside the timed loop, so the calibrated reps form a loop-carried dependency the optimizer cannot collapse.
- **Shared seed table.** A fixed 16-entry `SEEDS` table, identical across sizes and cells, so a cross-size or cross-cell comparison varies only the thing under test. Seeds drawn from `input[k % N]` would not do, since the harness fills the input differently per size.
- **One keep-alive fold per iteration.** The cell returns a single `u64` and the scaffold folds it once per outer iteration, never once per node, since an O(N) checksum inside the inner loop dilutes every ratio.
- **Setup against iteration, always measured.** `setup` is a required argument the scaffold brackets with counter reads, so the one-time build cost (S) cannot hide in untimed prep. Every cell reports both its per-iteration cost (I) and its setup cost (S), and the tier breakeven `k* = (S_b - S_a) / (I_a - I_b)` is computable from data the matrix always carries.
- **Reps-invariant fidelity digest.** Under calibration the reps count depends on timing, so the final `output` bytes vary with it and cannot witness fidelity across cells. The scaffold computes a separate digest on a fixed-seed, fixed-init single pass, on its own channel, which makes the cross-validation meaningful. The authoritative fidelity anchor stays the consumer's byte-exact cross-validation `#[test]`s, and the digest is a smoke check.
- **Cold, aliased-predictor regime.** `scaffold::cold_cycle` cycles M distinct programs so no single program's dispatch sequence is memorized by the branch predictor, which is the many-residuals-per-frame deployment shape, as opposed to the warm regime's memorized single program.

## Extras

### Status

The engine (`scaffold`, `decl`, `generate_all` and the canonical template), the `bench_matrix!` macro, and the `FfiBenchCall` ABI extension that surfaces the `setup_ns`, `first_ns` and `digest` columns, with the setup-against-iteration report table and the tier breakeven `k*`, are complete and tested. What remains is reporter refinements, floor-aware normalise and digest cross-validation, tracked in `TODO.md`.

### The three layers

`mockspace-bench-harness` is the transport: cdylib-per-variant isolation, the subprocess driver and the multi-axis spec to variant-crate codegen (`MatrixSpec`, `expand`, `render`, `generate`), which knows nothing about honest measurement. This crate is the discipline on top of it, the `scaffold` wrappers, the `decl` data model, `generate_all` and the single canonical template. The consumer owns its domain.

## Support

Feel free to contribute! If unsure about wasting work, the best practice is to throw in an issue describing what you'd do, and only then commit to writing a big PR, because chances are, it might not be something that belongs here. However, forks are always a valid choice and we'd encourage everyone to experiment and have their own takes on this. When doing this, do mind the license(s) though!

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the **Mozilla Public License 2.0**.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/dev/LICENSE)
