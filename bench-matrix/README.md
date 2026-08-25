# mockspace-bench-matrix

An opinionated layer on top of the mockspace bench harness for authoring a
*semantic benchmark matrix*: one bench built from many isolated cells, each timed
individually, swept across parameter axes, with the measurement discipline
provided rather than re-derived. It exists because that discipline is subtle: a
hand-rolled version of exactly this matrix shipped four distinct measurement
distortions that took a four-expert review to find, and the fix is to get the
scaffolding right once, upstream, and hand it to every consumer.

## What a "semantic matrix" is

A composited benchmark. You declare a *family* (say, "dispatch shape"), a set of
*cells* to compare within it (switch, fntable, threaded, ...), a *sweep* axis that
produces one bench per value (six program profiles), and a *size* sweep. Each
`(sweep value, cell, size)` becomes its own release cdylib, built under fat LTO
with one codegen unit and run in its own subprocess, so measuring one cell cannot
contaminate another. The harness already does that transport; this crate adds the
part that makes each individual measurement honest and each cell ergonomic to
author.

## The three layers

- **`mockspace-bench-harness`** is the transport: cdylib-per-variant isolation, the
  subprocess driver, the multi-axis spec to variant-crate codegen
  (`MatrixSpec`/`expand`/`render`/`generate`). It knows nothing about honest
  measurement.
- **`mockspace-bench-matrix`** (this crate) is the opinionated discipline on top:
  the `scaffold` wrappers, the `decl` data model, `generate_all`, and the single
  canonical template. This is where the disciplines a review panel validated are
  encoded once, compiled once, tested once.
- **The consumer** owns its domain: the program generator, the value semantics, and
  the cell functions themselves.

## The mechanism: cells are real typed functions, not strings

The pre-extraction approach spliced each cell as a Rust *string* (`prep` + `body`)
into a generated crate. That is stringly-typed code: no type checking until a
generated crate builds, no IDE support, no borrow-check feedback while authoring.
It looked mandatory because each cell compiles as its own cdylib, so the body
could not be an inline closure in the consumer crate.

The unlock: the string `body` was *already* a call back into the consumer crate
(`c::interpret(...)` is `consumer_crate::interpret`), and fat LTO already inlined
it across that exact boundary into the timed region. So a cell can be a real,
type-checked `pub fn` in the consumer crate, and each generated variant a one-line
call to it *by path*. Per-variant LTO inlines it into the isolated cdylib
identically. The measured machine code is the same; the authoring becomes real
Rust.

Two consequences worth stating plainly:

1. **The sibling crate never contains a cell body. It contains a call to the cell
   by path.** The generator needs the cell's path (a short string), not its body.
   Capturing a path is trivial; capturing a body is the trap (it either does
   filesystem IO from a macro or re-emits bodies as text and throws away the
   type-checking win). So the generator is a plain function over data: no
   proc-macro, no closure-token capture.
2. **The scaffold takes the cell as a generic `FnMut` parameter, never a `fn`
   pointer.** A generic parameter is monomorphized and inlined with no indirection.
   A `fn` pointer would reintroduce an indirect call the optimizer may decline to
   devirtualize, so the measured region would include a call the real deployment
   would not. That single signature choice is the whole isolation argument, and it
   is enforced by `scaffold::warm` / `scaffold::cold_cycle`.

## The disciplines the scaffold owns

Each of these was a distortion in the hand-rolled version. The scaffold makes them
impossible to get wrong:

- **Anti-hoist chain.** `scaffold` seeds `acc` from `output[0]`, folds one
  keep-alive per iteration, writes it back inside the timed loop, so the calibrated
  reps form a loop-carried dependency the optimizer cannot collapse. Consumers
  never touch it.
- **Shared seed table.** A fixed 16-entry `SEEDS` table, identical across sizes and
  cells, so a cross-size or cross-cell comparison varies only the thing under test.
  (The hand-rolled version drew seeds from `input[k % N]`, which the harness fills
  differently per size.)
- **Fold one keep-alive per iteration.** The cell returns a single `u64`; the
  scaffold folds it once per outer iteration, never once per node. (The hand-rolled
  version folded an O(N) checksum inside the inner loop, diluting every ratio.)
- **S-vs-I split, always measured.** `setup` is a required argument the scaffold
  brackets with counter reads, so the one-time build cost (S) can never hide in
  untimed prep. Every cell reports both its per-iteration cost (I) and its setup
  cost (S), so the tier breakeven `k* = (S_b - S_a) / (I_a - I_b)` is computable
  from data the matrix always carries.
- **Reps-invariant fidelity digest.** Under calibration the reps count is
  timing-dependent, so the final `output` bytes are reps-variant and cannot serve
  as a cross-cell fidelity witness. The scaffold computes a separate digest on a
  fixed-seed, fixed-init single pass (reps-invariant), on its own channel, so
  cross-validation is meaningful. The authoritative fidelity anchor remains the
  consumer's byte-exact cross-validation `#[test]`s; the digest is a smoke check.
- **Cold / aliased-predictor regime.** `scaffold::cold_cycle` cycles M distinct
  programs so no single program's dispatch sequence is memorized by the branch
  predictor: the many-residuals-per-frame deployment shape, as opposed to the warm
  regime's memorized single program.

## Authoring a matrix

The `bench_matrix!` macro emits the cell functions and the `matrix_decls()` data
from one declarative block. Write it in a `pub mod bench` of your consumer
*library* crate (the cells must be `pub` so the sibling variant crates name them by
path), then a four-line generator binary.

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

A feature-gated cell carries `#[feature = "..."]` between the tag and the closure;
those features flow into the sibling's carrier dependency. A cell that needs a
different construction declares its own `setup |..| -> St { .. }` before the
closure (the `direct`-style cell); the scaffold times that per-cell setup instead
of the shared one, and the cell reads that setup's own state type. `cold_cycle(M)`
cells take a third parameter `k` (the iteration index): `cell switch |s, k, seed|`.
`baseline` and `floor` name cells by tag and are checked at expansion, so a typo is
a compile error rather than a silent wrong baseline.

```rust
// the whole generator binary
fn main() -> std::io::Result<()> {
    let decls = bench_carrier::bench::matrix_decls();
    mockspace_bench_matrix::generate_all(&decls, std::path::Path::new("."))
}
```

`generate_all` writes one isolated variant crate per `(sweep value, cell)`, each a
one-line call to `scaffold::warm` (or `scaffold::cold_cycle`) naming your `setup`
and cell by path, and rewrites the `bench.toml` sections. The measurement logic
lives in the scaffold, so the generated files have nothing to get wrong.

### The hand-written equivalent

The macro is plain `macro_rules!`; it expands to `pub fn setup(..)`,
`pub fn cell_<tag>(..)`, and `pub fn matrix_decls() -> Vec<MatrixDecl>` exactly as
you would write them by hand, so the two forms are interchangeable. The hand form,
useful when a family needs shaping the macro grammar does not cover:

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

```rust
// the whole generator binary
fn main() -> std::io::Result<()> {
    let decls = bench_carrier::bench::matrix_decls();
    mockspace_bench_matrix::generate_all(&decls, std::path::Path::new("."))
}
```

The hand form is the exact expansion of the `bench_matrix!` block above; reach for
it only when a family needs shaping the grammar does not cover.

## Status

The engine (`scaffold`, `decl`, `generate_all`, the canonical template), the
`bench_matrix!` macro, and the `FfiBenchCall` ABI extension that surfaces the
`setup_ns` / `first_ns` / `digest` columns (and the setup-vs-iteration report table
with the tier breakeven `k*`) are all complete and tested. Remaining items are
reporter refinements (floor-aware normalise, digest cross-validation) tracked in
`TODO.md`. The carrier interpreter-composition matrix that produced this crate is
crate's first intended consumer and its regression test.

## A note on coding agents

We do not recommend using coding agents with this codebase. If you still choose to:
be aware of the environmental and social cost of large-scale model inference and
minimise it; only use an agent if you understand the architecture yourself; the
repository's agent instructions help but do not remove the need to correct the
agent frequently. Do this work yourself unless you know what you are doing and why.
