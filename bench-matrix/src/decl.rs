//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The data a `bench_matrix!` invocation produces and `generate_all` consumes.
//!
//! The load-bearing decision (see the crate README, section "The mechanism"): a
//! cell is referenced by its function PATH, never by its body. The macro emits a
//! typed `pub fn` per cell AND the path string that names it (via
//! `concat!(module_path!(), "::cell_switch")`), so the path can never drift from
//! the function. `generate_all` writes sibling crates that call the cell by that
//! path; the body stays in the consumer crate, compiled once, type-checked once,
//! inlined into each isolated cdylib by fat LTO. No proc-macro, no closure-token
//! capture, no filesystem IO from a macro.

/// Which scaffold entry point a matrix's variants call, and the per-regime shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Regime {
    /// Setup builds one state; the calibrated loop measures the op over it. The
    /// cell signature is `|&mut St, seed|`.
    Warm,
    /// Setup builds a state holding `m` distinct programs; the scaffold passes the
    /// iteration index so the cell selects `k % m` and the predictor cannot
    /// memorize a single program. The cell signature is `|&mut St, k, seed|`.
    ColdCycle(usize),
    /// The measured op sweeps the harness `input` byte stream itself (a
    /// throughput-over-a-byte-stream measurement, O(input) per call), rather than
    /// executing per seed. The cell signature is `|&mut St, input|` where `input`
    /// is `&[u8]`. For native-ceiling / run-over-input throughput benches.
    Stream,
}

impl Regime {
    /// The `scaffold::` function a variant of this regime calls.
    pub fn scaffold_fn(&self) -> &'static str {
        match self {
            Regime::Warm => "warm",
            Regime::ColdCycle(_) => "cold_cycle",
            Regime::Stream => "stream",
        }
    }
}

/// One measured variant within a bench: its tag, the path of the `pub fn` that is
/// the measured op, an optional per-cell setup path (overriding the family's
/// shared setup), and the carrier cargo features it needs.
#[derive(Clone, Debug)]
pub struct CellDecl {
    pub tag:        String,
    /// Full path of the measured-op `pub fn`, e.g.
    /// `vehje_bench_carrier::bench::carrier_dispatch::cell_switch`.
    pub op_path:    String,
    /// Per-cell setup override path; `None` uses the family's shared setup.
    pub setup_path: Option<String>,
    pub features:   Vec<String>,
}

/// The outer family sweep: one bench per value. An empty `values` (or a single
/// empty string) means an un-swept single bench. The value is passed to `setup`
/// as its first argument.
#[derive(Clone, Debug)]
pub struct SweepAxis {
    pub name:   String,
    pub values: Vec<String>,
}

/// One matrix family: a sweep of benches, each comparing the same set of cells.
#[derive(Clone, Debug)]
pub struct MatrixDecl {
    /// Bench-family prefix; each swept bench is `<name>_<sweep_value>`.
    pub name:        String,
    /// How a sibling crate names the consumer crate in `use`/paths, e.g.
    /// `vehje_bench_carrier`.
    pub crate_path:  String,
    /// The consumer-crate dependency line body for the variant `Cargo.toml`, with
    /// TOML inline-table braces escaped `{{`/`}}` and a `{carrier_features}` slot,
    /// e.g. `vehje-bench-carrier = {{ path = "../../carrier"{carrier_features} }}`.
    pub crate_dep:   String,
    /// Extra dependency lines emitted verbatim into every variant `Cargo.toml`
    /// (the bench-core / bench-macro / bench-matrix git-or-path deps).
    pub extra_deps:  Vec<String>,
    pub master_seed: String,
    pub sweep:       SweepAxis,
    pub sizes:       Vec<usize>,
    /// Cell tag that is the ratio denominator.
    pub baseline:    String,
    /// Cell tag to difference against for null-floor isolation (`None` = raw).
    pub floor:       Option<String>,
    pub regime:      Regime,
    /// Path of the family's shared setup `pub fn` (`fn(&str, usize) -> St`).
    pub setup_path:  String,
    pub cells:       Vec<CellDecl>,
}
