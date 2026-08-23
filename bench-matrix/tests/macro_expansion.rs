//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! End-to-end test of the `bench_matrix!` macro: that it expands to the same
//! `setup` / `cell_<tag>` / `matrix_decls()` surface the engine consumes, that the
//! `MatrixDecl` data it emits is correct, and that the generated cell functions
//! satisfy the scaffold's `FnMut` signature (the isolation-preserving form).

use mockspace_bench_matrix::{scaffold, Regime};

// A warm family standing in for a real consumer's `pub mod bench`: a shared state,
// a feature-gated cell, and a per-cell setup override returning its own state type.
mod warm_family {
    pub struct St {
        pub acc: u64,
        pub n:   usize,
    }
    // a per-cell setup returns a DIFFERENT state type; the cell must read it.
    pub struct DirectState {
        pub base: u64,
    }

    fn fold(s: &mut St, seed: u64) -> u64 {
        s.acc = s.acc.wrapping_add(seed);
        s.acc ^ (s.n as u64)
    }

    mockspace_bench_matrix::bench_matrix! {
        name: "demo_dispatch",
        crate_path: demo_consumer,
        crate_dep: "demo-consumer = {{ path = \"../c\"{carrier_features} }}",
        extra_deps: [ "mockspace-bench-core = { path = \"x\" }" ],
        seed: 0x5eed_d15b_a7c4_0002,
        sweep profile in ["real", "madd", "tight"],
        sizes: [64, 256, 1024],
        baseline: "switch",
        floor: "nullfloor",
        regime: warm,

        setup |profile: &str, n: usize| -> St {
            let _ = profile;
            St { acc: 0, n }
        }

        cell switch    |s, seed| { fold(s, seed) }
        cell fntable   #[feature = "ft"] |s, seed| { fold(s, seed).wrapping_add(1) }
        cell nullfloor |s, seed| { let _ = seed; let _ = s; 0 }

        // per-cell setup override: builds DirectState, and the cell reads &mut DirectState.
        cell direct
            #[feature = "jit"]
            setup |profile: &str, n: usize| -> DirectState { let _ = (profile, n); DirectState { base: 7 } }
            |s, seed| { s.base ^ seed }
    }
}

mod cold_family {
    pub struct ColdSt {
        pub pds: Vec<u64>,
    }

    mockspace_bench_matrix::bench_matrix! {
        name: "demo_cold",
        crate_path: demo_consumer,
        crate_dep: "demo-consumer = {{ path = \"../c\"{carrier_features} }}",
        extra_deps: [ ],
        seed: 0xabc_0001,
        sweep profile in ["real", "madd"],
        sizes: [64, 256],
        baseline: "switch",
        regime: cold_cycle(16),

        setup |profile: &str, n: usize| -> ColdSt {
            let _ = profile;
            ColdSt { pds: (0 .. n as u64).collect() }
        }

        // cold cells take the third param k (iteration index).
        cell switch |s, k, seed| { s.pds[k % s.pds.len().max(1)] ^ seed }
        cell null   |s, k, seed| { let _ = (k, seed, s); 0 }
    }
}

mod stream_family {
    pub struct St {
        pub r: Vec<u64>,
    }

    mockspace_bench_matrix::bench_matrix! {
        name: "demo_ceiling",
        crate_path: demo_consumer,
        crate_dep: "demo-consumer = {{ path = \"../c\"{carrier_features} }}",
        extra_deps: [ ],
        seed: 0xceil_0001,
        sweep profile in [],
        sizes: [64, 256, 1024],
        baseline: "interp",
        regime: stream,

        setup |profile: &str, n: usize| -> St {
            let _ = profile;
            St { r: vec![0u64; n] }
        }

        // stream cells sweep the harness input slice.
        cell interp |s, input| { let _ = &s.r; input.iter().fold(0u64, |h, &b| h.wrapping_add(b as u64)) }
        cell native |s, input| { let _ = &s.r; input.iter().map(|&b| b as u64).sum() }
    }
}

#[test]
fn stream_matrix_decls_and_scaffold_signature() {
    let decls = stream_family::matrix_decls();
    assert_eq!(decls.len(), 1);
    let d = &decls[0];
    assert_eq!(d.name, "demo_ceiling");
    assert_eq!(d.regime, Regime::Stream);
    assert!(d.sweep.values.is_empty(), "un-swept single bench");
    assert_eq!(d.cells.len(), 2);

    // the emitted stream cell drops into scaffold::stream (|&mut St, &[u8]|).
    let input = [4u8; 64];
    let mut out = [0u8; 8];
    let _ = scaffold::stream::<64, _, _, _>(
        &input,
        &mut out,
        |n| stream_family::setup("", n),
        stream_family::cell_interp,
    );
}

#[test]
fn warm_matrix_decls_are_correct() {
    let decls = warm_family::matrix_decls();
    assert_eq!(decls.len(), 1);
    let d = &decls[0];
    assert_eq!(d.name, "demo_dispatch");
    assert_eq!(d.crate_path, "demo_consumer");
    assert_eq!(d.master_seed, "0x5eed_d15b_a7c4_0002");
    assert_eq!(d.sweep.name, "profile");
    assert_eq!(d.sweep.values, vec!["real", "madd", "tight"]);
    assert_eq!(d.sizes, vec![64, 256, 1024]);
    assert_eq!(d.baseline, "switch");
    assert_eq!(d.floor.as_deref(), Some("nullfloor"));
    assert_eq!(d.regime, Regime::Warm);
    assert!(d.setup_path.ends_with("::warm_family::setup"), "setup_path was {}", d.setup_path);
    assert_eq!(d.extra_deps, vec!["mockspace-bench-core = { path = \"x\" }"]);

    assert_eq!(d.cells.len(), 4);
    let switch = &d.cells[0];
    assert_eq!(switch.tag, "switch");
    assert!(switch.op_path.ends_with("::warm_family::cell_switch"), "op_path was {}", switch.op_path);
    assert!(switch.setup_path.is_none(), "plain cell uses shared setup");
    assert!(switch.features.is_empty());

    let fntable = &d.cells[1];
    assert_eq!(fntable.tag, "fntable");
    assert_eq!(fntable.features, vec!["ft"]);
    assert!(fntable.setup_path.is_none());

    let direct = &d.cells[3];
    assert_eq!(direct.tag, "direct");
    assert_eq!(direct.features, vec!["jit"]);
    // the per-cell setup override sets a per-cell setup_path.
    assert!(
        direct.setup_path.as_deref().unwrap().ends_with("::warm_family::setup_direct"),
        "per-cell setup_path was {:?}",
        direct.setup_path
    );
}

#[test]
fn cold_matrix_decls_are_correct() {
    let decls = cold_family::matrix_decls();
    assert_eq!(decls.len(), 1);
    let d = &decls[0];
    assert_eq!(d.name, "demo_cold");
    assert_eq!(d.regime, Regime::ColdCycle(16));
    assert_eq!(d.floor, None, "no floor declared");
    assert_eq!(d.sweep.values, vec!["real", "madd"]);
    assert_eq!(d.cells.len(), 2);
    assert_eq!(d.cells[0].tag, "switch");
    assert!(d.extra_deps.is_empty());
}

#[test]
fn generated_cells_satisfy_the_scaffold_signature() {
    // the whole isolation argument is that a cell is a generic FnMut fn-item the
    // scaffold monomorphizes and inlines; proving the generated fn drops straight
    // into scaffold::warm confirms the emitted signature is that form.
    let input = [3u8; 64];
    let mut out = [0u8; 8];
    let m = scaffold::warm::<64, _, _, _>(
        &input,
        &mut out,
        |n| warm_family::setup("real", n),
        warm_family::cell_switch,
    );
    // digest is reps-invariant and folds the cell output, so it is not the init value.
    assert_ne!(m.digest, 0);

    // a per-cell-setup cell pairs its own setup with its cell, both by the scaffold.
    // `direct` carries `#[feature = "jit"]`, so the macro cfg-gates its setup+cell
    // on that feature; the direct call only typechecks when the feature is on
    // (`cargo test --features jit`). The MatrixDecl data for `direct` is still
    // emitted unconditionally, so `warm_matrix_decls_are_correct` covers it always.
    #[cfg(feature = "jit")]
    {
        let mut out2 = [0u8; 8];
        let _ = scaffold::warm::<64, _, _, _>(
            &input,
            &mut out2,
            |n| warm_family::setup_direct("real", n),
            warm_family::cell_direct,
        );
    }

    // a cold cell drops into scaffold::cold_cycle (three-arg).
    let mut out3 = [0u8; 8];
    let _ = scaffold::cold_cycle::<64, _, _, _>(
        &input,
        &mut out3,
        |n| cold_family::setup("real", n),
        cold_family::cell_switch,
    );
}
