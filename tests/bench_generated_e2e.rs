//! End-to-end proof of the generated bench path: a nested tree with
//! no consumer driver crate, one bench, one manifest-less arm, and a
//! hooks library, driven start to finish by `mock bench run`.
//!
//! This is the claim the whole consolidation rests on ("a consumer
//! with only byte-shaped benches owns zero Rust in the driver path"),
//! so it is established by running it rather than asserted: the tool
//! generates the arm manifest and the driver crate, builds both
//! against this repository via the `[build] mockspace` path spec,
//! runs a real (tiny) measurement, and promotes results, history and
//! the hook's own artifact.

use std::path::{Path, PathBuf};

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// The arm: the starter's hand-written export shape, no macro, so
/// the test exercises the generated manifest rather than bench-macro.
const ARM_LIB: &str = r#"
use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

fn plusone_impl(input: &u64, output: &mut u64) {
    *output = input.wrapping_add(1);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { plusone_impl(input, output); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"plusone\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
"#;

/// The hooks library: `after_cell` first asserts the cell's samples
/// and report are already staged (the ordering guarantee: moving the
/// hook before the writes turns this run red), then drops a marker
/// into the staged directory, and fails the `gate` bench so the
/// withheld-ledger behaviour is measured rather than documented.
const HOOKS_LIB: &str = r#"
use mockspace_bench_harness::driver::{AfterCell, CellVerdict, Hooks};

fn after_cell(cell: &AfterCell<'_>) -> CellVerdict {
    let stem = format!("{}_n{}", cell.config.sweep, cell.config.n);
    for staged in [format!("{stem}.csv"), format!("{stem}_report.md")] {
        assert!(
            cell.out_dir.join(&staged).is_file(),
            "after_cell fired before {staged} was staged"
        );
    }
    let marker = cell.out_dir.join(format!("{stem}_hook-marker.txt"));
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
    let root = std::env::temp_dir().join(format!("mockspace-bench-e2e-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
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
title = "Plus one"
workload = "default"
arms = ["plusone"]
points = [64]
master_seed = 7
"#,
        );
        write(
            &bench_dir.join(bench).join("arms").join("plusone").join("src").join("lib.rs"),
            ARM_LIB,
        );
    }
    write(&bench_dir.join("src").join("lib.rs"), HOOKS_LIB);

    // ── run it exactly as the command would. The gate bench's Fail
    // verdict must fail the exit code; every artifact must still be
    // on disk, which is what tells this failure apart from a run
    // that never completed. ──
    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let code = mockspace::bench::cmd(&cfg, &["run"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE),
        "the gate bench's Fail verdict must fail the run"
    );

    // ── everything the run promised is on disk, for both benches ──
    for bench in ["hash", "gate"] {
        let results = bench_dir.join("results").join(bench);
        for suffix in [".csv", ".meta.json", "_report.md", "_hook-marker.txt"] {
            let artifact = format!("{bench}_n64{suffix}");
            assert!(
                results.join(&artifact).is_file(),
                "missing promoted artifact {artifact} in {}",
                results.display()
            );
        }
    }
    // the metadata records the profile that was actually passed,
    // which this tree overrides; the old hardcoded literal would
    // have recorded opt-level=3 here
    let meta = std::fs::read_to_string(
        bench_dir.join("results").join("hash").join("hash_n64.meta.json"),
    )
    .unwrap();
    assert!(
        meta.contains("opt-level=0") && meta.contains("codegen-units=16"),
        "the recorded profile must be the overridden one: {meta}"
    );
    assert!(!meta.contains("opt-level=3"), "{meta}");

    // the accepted cell reaches the ledger; the gated-out one does not
    let history: PathBuf = bench_dir.join("history").join("hash").join("hash_n64.tsv");
    assert!(history.is_file(), "the accepted cell's ledger appends after promotion");
    assert!(
        std::fs::read_to_string(&history).unwrap().contains("plusone"),
        "the history rows carry the arm's exported name"
    );
    assert!(
        !bench_dir.join("history").join("gate").exists(),
        "a Fail verdict withholds the cell's history append"
    );
    // nothing wrote into the consumer's source area
    assert!(
        !bench_dir.join("hash").join("arms").join("plusone").join("Cargo.toml").exists(),
        "the generated arm manifest stays out of the consumer's tree"
    );

    std::fs::remove_dir_all(&root).ok();
}
