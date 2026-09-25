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

#[unsafe(no_mangle)]
pub extern "C" fn bench_output_size(_n: usize) -> usize {
    OUTPUT_SIZE
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
        bench_dir
            .join("results")
            .join("hash")
            .join("hash_n64.meta.json"),
    )
    .unwrap();
    assert!(
        meta.contains("opt-level=0") && meta.contains("codegen-units=16"),
        "the recorded profile must be the overridden one: {meta}"
    );
    assert!(!meta.contains("opt-level=3"), "{meta}");

    // the accepted cell reaches the ledger; the gated-out one does not
    let history: PathBuf = bench_dir.join("history").join("hash").join("hash_n64.tsv");
    assert!(
        history.is_file(),
        "the accepted cell's ledger appends after promotion"
    );
    assert!(
        std::fs::read_to_string(&history)
            .unwrap()
            .contains("plusone"),
        "the history rows carry the arm's exported name"
    );
    assert!(
        !bench_dir.join("history").join("gate").exists(),
        "a Fail verdict withholds the cell's history append"
    );
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

    std::fs::remove_dir_all(&root).ok();
}

/// An arm writing more than the routine it resolved to allocates is refused
/// before it runs, and only that arm: a bench beside it in the same tree, and
/// a fitting arm in the same bench, still run and are promoted. The arms are
/// the same code apart from the size they declare and their name, so the
/// refusal can only be that size. Without it the wide arm would write past the
/// harness's buffer on every call, which is how a bench left out of a tree's
/// routine table crashed its worker at exit with the heap corrupted.
#[test]
fn an_arm_declaring_another_output_size_than_its_routine_is_refused() {
    let repo = env!("CARGO_MANIFEST_DIR");
    let root = std::env::temp_dir().join(format!("mockspace-bench-size-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let mock_dir = root.join("mock");
    let bench_dir = mock_dir.join("benches");

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
    // The byte routine is eight bytes here, `[dispatch] out` being unset.
    for (bench, declared) in [("fits", "8"), ("wide", "32")] {
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
            &bench_dir
                .join(bench)
                .join("arms")
                .join("plusone")
                .join("src")
                .join("lib.rs"),
            &ARM_LIB.replace("OUTPUT_SIZE", declared),
        );
    }
    // Three more ways to be refused, one bench each: a size the arm does not
    // declare, no size export at all, and a stale hash with no size export,
    // where the hash has to be what is reported.
    let unexported = ARM_LIB.replace(
        "#[unsafe(no_mangle)]\npub extern \"C\" fn bench_output_size(_n: usize) -> usize {\n    OUTPUT_SIZE\n}\n",
        "",
    );
    assert!(!unexported.contains("bench_output_size"), "the export was taken out");
    let stale = unexported.replace("    abi_hash()\n", "    abi_hash() ^ 1\n");
    assert_ne!(stale, unexported, "the hash was changed");
    for (bench, lib) in [
        ("undeclared", ARM_LIB.replace("OUTPUT_SIZE", "0")),
        ("unexported", unexported.clone()),
        ("stale", stale),
    ] {
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
            &bench_dir
                .join(bench)
                .join("arms")
                .join("plusone")
                .join("src")
                .join("lib.rs"),
            &lib,
        );
    }
    // One bench holding both: the fitting arm and a wide one under its own name.
    write(
        &bench_dir.join("mixed").join("bench.toml"),
        r#"
title = "Plus one"
workload = "default"
arms = ["plusone", "wideone"]
points = [64]
master_seed = 7
"#,
    );
    for (arm, declared) in [("plusone", "8"), ("wideone", "32")] {
        write(
            &bench_dir
                .join("mixed")
                .join("arms")
                .join(arm)
                .join("src")
                .join("lib.rs"),
            &ARM_LIB
                .replace("OUTPUT_SIZE", declared)
                .replace("b\"plusone", &format!("b\"{arm}")),
        );
    }

    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let code = mockspace::bench::cmd(&cfg, &["run"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE),
        "a refused arm must fail the run"
    );
    for bench in ["fits", "mixed"] {
        assert!(
            bench_dir
                .join("results")
                .join(bench)
                .join(format!("{bench}_n64.csv"))
                .is_file(),
            "{bench}: the arm that fits its routine still runs and is promoted"
        );
    }
    assert!(
        bench_dir.join("history").join("fits").is_dir(),
        "the fitting bench reaches the ledger"
    );
    let mixed = std::fs::read_to_string(
        bench_dir
            .join("results")
            .join("mixed")
            .join("mixed_n64.csv"),
    )
    .unwrap();
    assert!(mixed.contains("plusone"), "the fitting arm was measured: {mixed}");
    assert!(
        !mixed.contains("wideone"),
        "the refused arm left no sample beside it: {mixed}"
    );
    for bench in ["undeclared", "unexported", "stale"] {
        assert!(
            !bench_dir.join("results").join(bench).join(format!("{bench}_n64.csv")).exists(),
            "{bench}: a refused arm is not measured"
        );
    }

    // Each refusal says why, which the run only prints. The arms are built by
    // now, so ask the preflight the driver ran, at the byte routine's 8 bytes.
    let arm = |bench: &str, arm: &str| {
        bench_dir
            .join("target")
            .join("mock-arms")
            .join(bench)
            .join(arm)
            .join("release")
            .join(format!(
                "{}{arm}{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ))
            .display()
            .to_string()
    };
    let refusal = |bench: &str, name: &str| {
        mockspace_bench_harness::harness::preflight_variant(&arm(bench, name), 64, 8)
            .expect_err(&format!("{bench}/{name} must be refused"))
    };
    assert!(
        mockspace_bench_harness::harness::preflight_variant(&arm("fits", "plusone"), 64, 8)
            .is_ok(),
        "the control: the fitting arm passes the same preflight"
    );
    let wide = refusal("wide", "plusone");
    assert!(wide.contains("writes 32 bytes") && wide.contains("allocates 8"), "{wide}");
    let undeclared = refusal("undeclared", "plusone");
    assert!(undeclared.contains("declares no size 64"), "{undeclared}");
    let unexported = refusal("unexported", "plusone");
    assert!(
        unexported.contains("missing bench_output_size") && unexported.contains("hand-written"),
        "{unexported}"
    );
    let stale = refusal("stale", "plusone");
    assert!(stale.contains("ABI hash mismatch"), "{stale}");
    assert!(
        !stale.contains("bench_output_size"),
        "the hash is checked before the size export: {stale}"
    );
    assert!(
        !bench_dir
            .join("results")
            .join("wide")
            .join("wide_n64.csv")
            .exists(),
        "the bench whose arm declares 32 bytes against an 8-byte routine is refused"
    );
    assert!(
        !bench_dir.join("history").join("wide").exists(),
        "and nothing of it reaches the ledger"
    );

    std::fs::remove_dir_all(&root).ok();
}
