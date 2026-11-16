//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The size an arm declares against the routine it resolved to, end to end
//! through `mock bench run`: a refused arm is refused alone, and says why.

use std::path::{Path, PathBuf};

use bench_e2e::{ARM_LIB, freeze_all, runs_of, write};

#[path = "common/bench_e2e.rs"]
mod bench_e2e;

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
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = root_guard.path().to_path_buf();
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
            &lib,
        );
    }
    // One bench holding both: the fitting arm and a wide one under its own name.
    write(
        &bench_dir.join("mixed").join("bench.toml"),
        r#"
kind = "snapshot"
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

    // `--keep-builds`: the preflight below reads the arms the run built, which a
    // run removes otherwise.
    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    freeze_all(&cfg);
    let code = mockspace::bench::cmd(&cfg, &["run", "--keep-builds"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE),
        "a refused arm must fail the run"
    );
    for bench in ["fits", "mixed"] {
        let runs = runs_of(&bench_dir, bench);
        assert!(
            runs.len() == 1 && runs[0].join("samples.csv").is_file(),
            "{bench}: the arm that fits its routine still runs and is promoted: {runs:?}"
        );
    }
    assert!(
        runs_of(&bench_dir, "fits")[0].join("summary.tsv").is_file(),
        "the fitting bench carries its summary"
    );
    let mixed = std::fs::read_to_string(runs_of(&bench_dir, "mixed")[0].join("samples.csv")).unwrap();
    assert!(mixed.contains("plusone"), "the fitting arm was measured: {mixed}");
    assert!(
        !mixed.contains("wideone"),
        "the refused arm left no sample beside it: {mixed}"
    );
    for bench in ["undeclared", "unexported", "stale"] {
        assert!(
            runs_of(&bench_dir, bench).iter().all(|r| !r.join("samples.csv").exists()),
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
        runs_of(&bench_dir, "wide").is_empty(),
        "the bench whose arm declares 32 bytes against an 8-byte routine is refused, and \
         nothing of it is promoted"
    );

    // A run without the flag removes what it built. One bench that runs clean
    // succeeds, and then its arm's link under the bench tree and the driver in
    // the target cargo built it in are gone.
    let shared = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| mock_dir.join("target"));
    let driver = mockspace::bench_gen::driver_name(&bench_dir);
    let driver_files = |dir: &Path| -> Vec<String> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&driver) || n.starts_with(&driver.replace('-', "_")))
            .collect()
    };
    assert!(!driver_files(&shared.join("release")).is_empty(), "the kept run left its driver");
    let code = mockspace::bench::cmd(&cfg, &["run", "fits"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::SUCCESS),
        "a plain run of the bench that fits must succeed"
    );
    assert!(
        !bench_dir.join("target").join("mock-arms").join("fits").exists(),
        "the arm's link stays after a plain run"
    );
    let left: Vec<String> = ["release", "release/deps"].iter().flat_map(|d| driver_files(&shared.join(d))).collect();
    assert!(left.is_empty(), "the driver stays after a plain run: {left:?}");

    // the refused arms still fail a run of everything, which then removes the
    // rest of what the kept run built
    let code = mockspace::bench::cmd(&cfg, &["run"]);
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::FAILURE),
        "the refused arms fail a plain run of everything"
    );
    assert!(!bench_dir.join("target").exists(), "the per-arm links stay after a plain run");

}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
