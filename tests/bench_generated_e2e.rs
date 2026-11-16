//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! End-to-end proof of the generated bench path: a nested tree with
//! no consumer driver crate, one bench, one manifest-less arm, and a
//! hooks library, driven start to finish by `mock bench run`.
//!
//! This is the claim the whole consolidation rests on ("a consumer
//! with only byte-shaped benches owns zero Rust in the driver path"),
//! so it is established by running it rather than asserted: the tool
//! generates the arm manifest and the driver crate, builds both
//! against this repository via the `[build] mockspace` path spec,
//! runs a real (tiny) measurement, and promotes each cell's run directory
//! with the hook's own artifact in it. Run twice, the second run lands
//! beside the first and replaces nothing.

use bench_e2e::{ARM_LIB, freeze_all, runs_of, write};

#[path = "common/bench_e2e.rs"]
mod bench_e2e;

/// The hooks library: `after_cell` first asserts the cell's samples
/// and report are already staged (the ordering guarantee: moving the
/// hook before the writes turns this run red), then drops a marker
/// into the staged directory, and fails the `gate` bench so the
/// withheld-ledger behaviour is measured rather than documented.
const HOOKS_LIB: &str = r#"
use mockspace_bench_harness::driver::{AfterCell, CellVerdict, Hooks};

fn after_cell(cell: &AfterCell<'_>) -> CellVerdict {
    for staged in ["samples.csv", "meta.json", "report.md"] {
        assert!(
            cell.out_dir.join(&staged).is_file(),
            "after_cell fired before {staged} was staged"
        );
    }
    let marker = cell.out_dir.join("hook-marker.txt");
    std::fs::write(&marker, "after_cell ran\n").expect("marker writes");
    if cell.config.bench == "gate" {
        CellVerdict::Fail("gated: this cell must not reach the ledger".into())
    } else {
        CellVerdict::Note("marker written".into())
    }
}

pub fn hooks() -> Hooks {
    Hooks {
        after_cell: Some(after_cell),
        ..Hooks::default()
    }
}
"#;

#[test]
fn a_config_only_nested_tree_runs_end_to_end_through_the_generated_driver() {
    let repo = env!("CARGO_MANIFEST_DIR");
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = root_guard.path().to_path_buf();
    let mock_dir = root.join("mock");
    let bench_dir = mock_dir.join("benches");

    // ── the whole consumer authorship for this tree ──
    write(
        &bench_dir.join("bench.toml"),
        &format!(
            r#"
[build]
mockspace = '{{ path = "{repo}" }}'
opt-level = 0
lto = "off"
codegen-units = 16

[timing]
passes = 1
runs_per_pass = 20
batch_size = 5
harness_runs = 1
cooldowns_ms = [0]
"#
        ),
    );
    for bench in ["hash", "gate"] {
        write(
            &bench_dir.join(bench).join("bench.toml"),
            r#"
kind = "snapshot"
title = "Plus one"
workload = "default"
arms = ["plusone"]
points = [64]
master_seed = 7
"#,
        );
        write(
            &bench_dir
                .join(bench)
                .join("arms")
                .join("plusone")
                .join("src")
                .join("lib.rs"),
            &ARM_LIB.replace("OUTPUT_SIZE", "8"),
        );
    }
    write(&bench_dir.join("src").join("lib.rs"), HOOKS_LIB);

    // ── run it exactly as the command would. The gate bench's Fail
    // verdict must fail the exit code; every artifact must still be
    // on disk, which is what tells this failure apart from a run
    // that never completed. ──
    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    freeze_all(&cfg);
    let code = mockspace::bench::cmd(&cfg, &["run"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE),
        "the gate bench's Fail verdict must fail the run"
    );

    // ── everything the run promised is on disk, for both benches, in one
    // run directory each ──
    for bench in ["hash", "gate"] {
        let runs = runs_of(&bench_dir, bench);
        assert_eq!(runs.len(), 1, "{bench}: one run, {runs:?}");
        for file in ["samples.csv", "meta.json", "report.md", "hook-marker.txt"] {
            assert!(
                runs[0].join(file).is_file(),
                "missing promoted artifact {file} in {}",
                runs[0].display()
            );
        }
        assert_eq!(
            mockspace_bench_harness::layout::place(
                &runs[0].join("samples.csv").strip_prefix(&bench_dir).unwrap(),
                |_| true
            ),
            mockspace_bench_harness::layout::Placement::InLayout
        );
    }
    let hash_run = runs_of(&bench_dir, "hash").remove(0);
    // the metadata records the profile that was actually passed,
    // which this tree overrides; the old hardcoded literal would
    // have recorded opt-level=3 here, and it names its run and class
    let meta = std::fs::read_to_string(hash_run.join("meta.json")).unwrap();
    assert!(
        meta.contains("opt-level=0") && meta.contains("codegen-units=16"),
        "the recorded profile must be the overridden one: {meta}"
    );
    assert!(!meta.contains("opt-level=3"), "{meta}");
    let run_id = hash_run.file_name().unwrap().to_string_lossy().into_owned();
    let class = hash_run.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
    assert!(meta.contains(&format!("\"run\":\"{run_id}\"")), "{meta}");
    assert!(meta.contains(&format!("\"machine_class\":\"{class}\"")), "{meta}");

    // the accepted cell carries its summary; the gated-out one does not
    let summary = hash_run.join("summary.tsv");
    assert!(
        std::fs::read_to_string(&summary).unwrap().contains("plusone"),
        "the summary rows carry the arm's exported name"
    );
    // a snapshot's rows name the lock they measured, and its report says what
    // kind of bench it is and how its history reads
    let lock = std::fs::read(bench_dir.join("hash/snapshot.lock")).unwrap();
    let source = {
        use sha2::Digest;
        let hex: String = sha2::Sha256::digest(&lock).iter().map(|b| format!("{b:02x}")).collect();
        hex[.. 12].to_string()
    };
    let rows = mockspace_bench_harness::history::parse_summary(&std::fs::read_to_string(&summary).unwrap());
    assert!(!rows.is_empty() && rows.iter().all(|r| r.source == source), "{rows:?}");
    let report = std::fs::read_to_string(hash_run.join("report.md")).unwrap();
    assert!(report.contains(&format!("Kind: snapshot, source `{source}`")), "{report}");
    assert!(
        !runs_of(&bench_dir, "gate")[0].join("summary.tsv").exists(),
        "a Fail verdict withholds the cell's summary"
    );
    assert!(!bench_dir.join("history").exists(), "no ledger is written beside the runs");

    // ── a second run lands beside the first and replaces nothing ──
    let first_samples = std::fs::read(hash_run.join("samples.csv")).unwrap();
    let _ = mockspace::bench::cmd(&cfg, &["run", "hash"]);
    let runs = runs_of(&bench_dir, "hash");
    assert_eq!(runs.len(), 2, "the re-run is a second run directory: {runs:?}");
    assert_eq!(
        std::fs::read(hash_run.join("samples.csv")).unwrap(),
        first_samples,
        "the earlier run's samples stand untouched"
    );
    assert!(runs[1].join("summary.tsv").is_file(), "{:?}", runs[1]);
    // the second run at the unchanged snapshot is written, and stands at the same source point
    let again = mockspace_bench_harness::history::parse_summary(&std::fs::read_to_string(runs[1].join("summary.tsv")).unwrap());
    assert!(!again.is_empty() && again.iter().all(|r| r.source == source), "{again:?}");
    let index = std::fs::read_to_string(bench_dir.join("results/INDEX.md")).unwrap();
    assert!(index.contains("2 run(s)"), "{index}");

    // nothing wrote into the consumer's source area
    assert!(
        !bench_dir
            .join("hash")
            .join("arms")
            .join("plusone")
            .join("Cargo.toml")
            .exists(),
        "the generated arm manifest stays out of the consumer's tree"
    );

}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
