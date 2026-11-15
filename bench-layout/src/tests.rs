//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Where `p` stands in a tree where every sibling meta is present.
fn at(p: &str) -> Placement {
    place(Path::new(p), |_| true)
}

/// Where `p` stands in a tree holding exactly `present`.
fn among(p: &str, present: &[&str]) -> Placement {
    place(Path::new(p), |q| present.iter().any(|x| Path::new(x) == q))
}

fn legacy(bench: &str, sweep: &str, n: u64, part: Part) -> Placement {
    Placement::Legacy(Legacy {
        bench: bench.into(),
        cell: Cell {
            sweep: sweep.into(),
            n,
        },
        part,
    })
}

const RUN: &str = "results/hash/hash_n64/linux-x86_64-c4-container-intel/20261115T093000Z-1a2b3c4";

#[test]
fn a_cell_name_round_trips_and_takes_the_last_n() {
    let c = Cell::parse("bit_n_ops_n64").unwrap();
    assert_eq!(c.sweep, "bit_n_ops");
    assert_eq!(c.n, 64);
    assert_eq!(c.name(), "bit_n_ops_n64");
    for bad in ["hash", "hash_n", "_n64", "hash_nx", "hash_n6x4", ".h_n1"] {
        assert_eq!(Cell::parse(bad), None, "`{bad}`");
    }
}

#[test]
fn a_run_id_is_the_utc_start_and_the_commit() {
    // 2026-11-15T09:30:00Z.
    assert_eq!(run_id(1_794_735_000, "1a2b3c4-dirty"), "20261115T093000Z-1a2b3c4-dirty");
    assert_eq!(run_id(0, ""), "19700101T000000Z-nogit");
    assert_eq!(run_id(0, "unknown"), "19700101T000000Z-nogit");
    assert_eq!(run_id(951_782_400, "a/b"), "20000229T000000Z-a_b", "a leap day, and a slash");
    assert!(is_run_id(&run_id(1_794_735_000, "abc")));
    assert!(is_run_id("20261115T093000Z-abc-2"), "a suffix taken on a collision");
    for bad in ["20261115T093000Z", "20261115T093000Z-", "2026111T093000Z-a", "20261115-093000Z-a"] {
        assert!(!is_run_id(bad), "`{bad}`");
    }
}

#[test]
fn a_machine_class_is_the_shape_the_harness_writes() {
    for good in ["linux-x86_64-c16-bare-amd", "macos-aarch64-cx-bare-apple", "other-other-c2-container-other"] {
        assert!(is_machine_class(good), "`{good}`");
    }
    for bad in ["linux-x86_64-c16-bare", "Linux-x86_64-c16-bare-amd", "linux-x86_64-16-bare-amd", "linux-x86_64-c16-vm-amd", ""] {
        assert!(!is_machine_class(bad), "`{bad}`");
    }
}

#[test]
fn every_file_of_a_run_directory_is_in_the_layout() {
    for f in RUN_FILES {
        assert_eq!(at(&format!("{RUN}/{f}")), Placement::InLayout, "{f}");
    }
    assert_eq!(at(&format!("{RUN}/_hook-marker.txt")), Placement::InLayout, "a hook's evidence");
    assert_eq!(at("results/INDEX.md"), Placement::InLayout);
    assert_eq!(at("results/void/123-4/hash/x.csv"), Placement::InLayout);
    assert_eq!(at("results/.inflight/123-4/hash/x.csv"), Placement::InLayout);
}

/// The failing side of the one above: each part of a run path being wrong
/// takes the file out of the layout.
#[test]
fn a_run_path_wrong_in_any_part_is_not_in_the_layout() {
    for bad in [
        format!("{RUN}/deeper/samples.csv"),
        "results/hash/hash_n64/linux-x86_64-c4-container-intel/latest/samples.csv".to_string(),
        "results/hash/hash_n64/my-laptop/20261115T093000Z-1a2b3c4/samples.csv".to_string(),
        "results/hash/hash64/linux-x86_64-c4-container-intel/20261115T093000Z-1a2b3c4/samples.csv".to_string(),
        "results/hash/samples.csv".to_string(),
        "results/notes.md".to_string(),
    ] {
        assert_eq!(at(&bad), Placement::Stray(Stray::InResults), "{bad}");
    }
}

#[test]
fn the_harness_flat_results_translate() {
    assert_eq!(at("results/hash/hash_n64.csv"), legacy("hash", "hash", 64, Part::Samples));
    assert_eq!(at("results/hash/hash_n64.meta.json"), legacy("hash", "hash", 64, Part::Meta));
    assert_eq!(at("results/hash/hash_n64_findings.md"), legacy("hash", "hash", 64, Part::Report));
    assert_eq!(
        at("results/warm/width-l1_n80003_report.md"),
        legacy("warm", "width-l1", 80003, Part::Report)
    );
}

#[test]
fn a_root_driver_s_outputs_and_both_ledgers_translate() {
    assert_eq!(at("atomic_inc_n1024.csv"), legacy("atomic_inc", "atomic_inc", 1024, Part::Samples));
    assert_eq!(at("atomic_inc_n1024.meta.json"), legacy("atomic_inc", "atomic_inc", 1024, Part::Meta));
    assert_eq!(at(".bench_history/hash_n64.tsv"), legacy("hash", "hash", 64, Part::History));
    assert_eq!(at("history/warm/width-l1_n80003.tsv"), legacy("warm", "width-l1", 80003, Part::History));
}

#[test]
fn outputs_anywhere_else_are_strays() {
    assert_eq!(at("runs/run1/rsb_n256.csv"), Placement::Stray(Stray::Outside));
    assert_eq!(at("frame_wide/heavy_n65536_findings.md"), Placement::Stray(Stray::Outside));
    assert_eq!(at("frame_wide/notes.meta.json"), Placement::NotOutput, "the harness never wrote that name");
    assert_eq!(at("frame_wide/.bench_history/heavy_n1.tsv"), Placement::Stray(Stray::Ledger));
    assert_eq!(at(".bench_history/notes.txt"), Placement::Stray(Stray::Ledger));
    assert_eq!(at("history/hash_n64.tsv"), Placement::Stray(Stray::Ledger));
}

#[test]
fn sources_and_notes_are_not_outputs() {
    for p in ["bench.toml", "hash/arms/fnv/src/lib.rs", "hash/bench.toml", "README.md", "FINDINGS.md", "support/x/Cargo.toml"] {
        assert_eq!(at(p), Placement::NotOutput, "{p}");
    }
}

#[test]
fn runs_are_found_oldest_first_and_odd_directories_are_passed_over() {
    let root = mockspace_manifest::scratch::held();
    let _ = std::fs::remove_dir_all(&root);
    let cell = Cell {
        sweep: "hash".into(),
        n:     64,
    };
    let m = "linux-x86_64-c4-container-intel";
    for run in ["20261115T093000Z-b", "20261114T093000Z-a"] {
        std::fs::create_dir_all(root.join(run_rel("hash", &cell, m, run))).unwrap();
    }
    std::fs::create_dir_all(root.join("hash/hash_n64/not-a-class/20261115T093000Z-b")).unwrap();
    std::fs::create_dir_all(root.join("void/1-2/hash")).unwrap();
    let found: Vec<String> = runs(&root).into_iter().map(|r| r.run).collect();
    assert_eq!(found, ["20261114T093000Z-a", "20261115T093000Z-b"]);
    std::fs::remove_dir_all(&root).ok();
}

/// Inside a crate (under `support/`, under `arms/`, or below a directory with a
/// `Cargo.toml`), a file named like an output is one only beside its meta:
/// input data and golden files there carry the shape and are not outputs.
#[test]
fn inside_a_crate_an_output_shaped_file_counts_only_beside_its_meta() {
    let engine = ["engine/Cargo.toml"];
    for (p, present) in [
        ("support/data/input_n64.csv", &[][..]),
        ("hash/arms/fnv/tests/golden_n64_report.md", &[][..]),
        ("engine/data/table_n64.csv", &engine[..]),
    ] {
        assert_eq!(among(p, present), Placement::NotOutput, "{p}");
    }
    assert_eq!(
        among("support/data/input_n64.csv", &["support/data/input_n64.meta.json"]),
        Placement::Stray(Stray::Outside)
    );
    assert_eq!(among("frame_wide/heavy_n65536.meta.json", &[]), Placement::Stray(Stray::Outside), "a meta is its own anchor");
}

/// Outside a crate, a runner of its own wrote bare CSVs with no meta beside
/// them, and those are outputs: at the bench root they translate, anywhere
/// else they are strays. A `Cargo.toml` at the bench root does not make the
/// whole tree a crate.
#[test]
fn outside_a_crate_a_bare_output_is_still_an_output() {
    assert_eq!(among("atomic_inc_n1024.csv", &[]), legacy("atomic_inc", "atomic_inc", 1024, Part::Samples));
    assert_eq!(among("atomic_inc_n1024.csv", &["Cargo.toml"]), legacy("atomic_inc", "atomic_inc", 1024, Part::Samples));
    assert_eq!(among("runs/run1/rsb_n256.csv", &[]), Placement::Stray(Stray::Outside));
    assert_eq!(among("frame_wide/heavy_n65536_findings.md", &[]), Placement::Stray(Stray::Outside));
    // Under `results/` and in a ledger the meta is never asked: those are
    // outputs by where they sit.
    assert_eq!(among("results/hash/hash_n64.csv", &[]), legacy("hash", "hash", 64, Part::Samples));
    assert_eq!(among(".bench_history/hash_n64.tsv", &[]), legacy("hash", "hash", 64, Part::History));
}
