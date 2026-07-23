# mockspace-bench-matrix TODO

The engine (scaffold, decl, generate_all, canonical template) is complete and
tested. What follows turns it into the full ergonomic, honest-by-construction
surface the design calls for. Ordered by leverage. The design reference is the
vehje repo's `mock/research/202607230922_carrier-matrix-result-review-panel/`
(`muratori_extraction_design.md` for this crate, the four panel files for why each
discipline matters).

## 1. Extend `FfiBenchCall` and surface the S / first-touch / digest columns — DONE

Landed: `FfiBenchCall` now carries `{ run_ticks, setup_ticks, first_ticks, digest }`,
`abi_hash()` folds the four-field layout (stale variant dylibs rejected at load),
`timed!`/`timed_calibrated!` zero-fill the new fields, `Measured::into_ffi`
populates all four, `Sample` and both CSV read/write pairs carry them (columns
appended so older CSVs still load), the analysis aggregate computes
`mean_setup_ns`/`mean_first_ns`, and the report gained a setup-vs-iteration table
with the tier breakeven `k* = (S_b - S_a) / (I_a - I_b)`. The panel's number-one
finding (S hidden in untimed prep) is closed. Original spec kept below for record.

The scaffold already computes `setup_ticks`, `first_ticks`, and a reps-invariant
`digest` in `Measured`, but `Measured::into_ffi` drops them because the shipping
`FfiBenchCall` carries only `run_ticks`. This item makes them visible.

- **bench-core:** change `FfiBenchCall` from `{ run_ticks }` to
  `{ run_ticks, setup_ticks, first_ticks, digest }`. Update `abi_hash()` to cover
  the new layout (the ABI hash exists precisely to catch this drift on load). Update
  the `timed!` / `timed_calibrated!` constructors that build `FfiBenchCall`
  (`bench-core/src/lib.rs` around lines 496 and 553) to fill the new fields with
  zeros (they measure only `run_ticks`; the matrix scaffold fills the rest).
- **bench-matrix:** change `Measured::into_ffi` to populate all four fields.
- **bench-harness:** in the analysis/reporting layer, read the new fields and emit
  `setup_ns` and `algo_ns_first` columns in the per-bench CSV/report.
- **Consequence:** every family then reports its S term alongside its I term, and
  the tier breakeven `k* = (S_b - S_a) / (I_a - I_b)` is computable from the matrix
  directly. This is the review panel's number-one finding (S hidden in untimed
  prep) fully closed. It is additive plus one struct field; the ABI-hash bump means
  all consumers rebuild, which is the intended safety behaviour.

## 2. The `bench_matrix!` declarative macro — DONE

Landed in `src/macros.rs`. Two regime arms (`warm` 2-arg cells, `cold_cycle(M)`
3-arg cells), optional per-cell `#[feature = "..."]`, optional per-cell `setup`
override (with a `__bm_state!` picker so the override cell reads its own state
type), and optional `floor`. `baseline`/`floor` name cells by tag. It emits
`pub fn setup`, `pub fn cell_<tag>`, optional `pub fn setup_<tag>`, and
`pub fn matrix_decls() -> Vec<MatrixDecl>` with op/setup paths built by
`format!("{}::cell_{}", module_path!(), ...)`. `tests/macro_expansion.rs` covers
both regimes, the feature gate, the per-cell setup override, and that the emitted
cell fns drop straight into `scaffold::warm` / `scaffold::cold_cycle`.

Decision record: used `paste` (re-exported as `mockspace_bench_matrix::paste`, so
consumers need no direct dep) for the `cell_<tag>` / `setup_<tag>` ident
concatenation `macro_rules!` cannot do. This matches the design doc's settled
expansion (`cell_<tag>` free fns) and keeps the hand-written README form
byte-interchangeable with the macro expansion. The module-per-cell alternative was
rejected: it would diverge the emitted symbol names from the documented hand-form
and bet on glob-import resolution of call-site body tokens.

Not yet covered (open, low priority): **multi-sweep.** The entropy grid is a 2D
`op_correlation x locality_window` product; the macro's `sweep` is a single axis.
Either add cartesian multi-sweep, or keep it a single flattened sweep with
composite values. The entropy-grid family can be hand-authored until then.

## 3. Floor-aware normalise (null-floor differencing) in the harness reporter — DONE

Wired end to end: `MatrixDecl::floor` -> `MatrixSpec::floor_contains` ->
`render_bench_section` resolves the floor tag to its variant name and emits
`floor = "<variant>"` in the `[bench.<bench>.normalise]` block -> `NormaliseSection`
/ `BenchConfig::normalise_floor` parse it -> the driver calls `DataSet::with_floor`
-> the report's `× base` ratio column becomes floor-differenced
`(variant - floor) / (baseline - floor)` with an explaining note. So the ratio
isolates pure dispatch cost above the null-dispatch floor cell (the panel's fix for
the hand-rolled version that wired `subtract` at the wrong baseline). Tested:
`matrix::bench_section_emits_resolved_floor` (plumbing) and
`report::floor_differences_the_ratio_against_the_named_cell` (differencing math +
the no-floor raw-ratio fallback).

## 4. Cross-validate on `digest`, not `output`

Once item 1 lands, the harness's cross-variant fidelity check should read
`FfiBenchCall.digest` (reps-invariant, meaningful under calibration) instead of the
reps-variant `output` bytes (which the panel showed makes the current
`MAY_DIFFER = false` check inert). This is a small harness change gated on item 1.

## 5. `first_touch` reporting and the k-ladder regime

- `first_touch` is not a separate measurement: the scaffold already emits
  `first_ticks` on every warm call. It is a reporting choice (headline the
  first-rep column). Add it as a reporter flag once item 1 surfaces `algo_ns_first`.
- The full `total(k) = S + k*I` geometric k-ladder with least-squares fit was
  ranked high-effort and deferred by the panel (its in-process rungs are not cold;
  it needs process-per-rung). The scaffold shape accommodates it as a future
  `Regime::KLadder(vec![1,2,4,..,128])` that emits a per-rung column. Not needed
  until tier-selection thresholds become a runtime feature.

## 6. Dogfood: migrate the vehje carrier matrix onto this crate

The carrier interpreter-composition matrix (`vehje/mock/benches/src/bin/gen_matrix.rs`
plus the carrier crate's cells) is this crate's first consumer and its regression
test. Migrating it: move the string `prep` fragments to typed `setup` bodies and the
string `body` fragments to typed `cell` bodies inside a `pub mod bench` of the
carrier *library* crate, and shrink `gen_matrix.rs` to the four-line generator. The
carrier's byte-exact cross-validation `#[test]`s stay put as the authoritative
fidelity anchor. Do this after the current PMU bench run's results are captured, so
the migration is validated against a known-good matrix rather than concurrently with
producing one.

## Notes on placement and process

- This crate is v1 (the shipping line). v1 work is done directly on feature branches
  without the mockspace design-round ceremony (only v2 uses that flow).
- The transport (`harness::{expand, render, generate}`) is correct and must not
  change; every item above sits on top of it or in the reporting layer.
