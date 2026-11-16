//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A tracking bench's source points, end to end through `mock bench run`.
//!
//! The arm reaches a crate outside the bench tree through a support crate and
//! a crate that inherits it from its workspace, and a root `[bench.<name>]`
//! section's variant reaches the same crate from its own directory. Four runs: one, the same again, one after that crate's source changed, and
//! one after it was put back, as a run at an older revision is. Every run is
//! written; the two at unchanged code stand at one source, the changed code
//! at another, and the run on the code put back lands under the first point
//! again. Each row names the system that ran it.

use std::path::{Path, PathBuf};

use bench_e2e::{ARM_LIB, write};

#[path = "common/bench_e2e.rs"]
mod bench_e2e;

/// Every run directory of the bench's cell, oldest first, with its summary rows.
fn runs(bench_dir: &Path, bench: &str) -> Vec<(PathBuf, Vec<mockspace_bench_harness::history::HistoryEntry>)> {
    mockspace_bench_harness::layout::runs(&bench_dir.join("results"))
        .into_iter()
        .filter(|r| r.bench == bench)
        .map(|r| {
            (
                r.dir.clone(),
                mockspace_bench_harness::history::run_rows(&r),
            )
        })
        .collect()
}

#[test]
fn a_tracking_bench_gathers_every_run_at_its_source_point() {
    let repo = env!("CARGO_MANIFEST_DIR");
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let root = root_guard.path().to_path_buf();
    let mock_dir = root.join("mock");
    let bench_dir = mock_dir.join("benches");

    // A root `[bench.sec]` section beside the composed bench: its variant has
    // a manifest of its own and builds in its own crate directory, and its
    // source is looked up by that directory. The entry is a path so the build
    // is pointed there whatever target directory a cargo configuration names.
    write(
        &bench_dir.join("bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{repo}\" }}'\nopt-level = 0\nlto = \"off\"\ncodegen-units = 16\n\n\
             [timing]\npasses = 1\nruns_per_pass = 20\nbatch_size = 5\nharness_runs = 1\ncooldowns_ms = [0]\n\n\
             [bench.sec]\nkind = \"tracking\"\ntitle = \"Section\"\nworkload = \"default\"\nvariants = [\"./variants/secone/target/release/secone\"]\nsizes = [64]\nmaster_seed = 7\n"
        ),
    );
    write(
        &bench_dir.join("variants/secone/Cargo.toml"),
        &format!(
            "[workspace]\n[package]\nname = \"secone\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n\
             [dependencies]\nmockspace-bench-core = {{ path = \"{repo}/bench-core\" }}\nengine = {{ path = \"../../../crates/engine\" }}\n"
        ),
    );
    write(
        &bench_dir.join("variants/secone/src/lib.rs"),
        &ARM_LIB.replace("OUTPUT_SIZE", "8").replace("b\"plusone", "b\"secone"),
    );
    write(
        &bench_dir.join("live/bench.toml"),
        "kind = \"tracking\"\ntitle = \"Plus one\"\nworkload = \"default\"\narms = [\"plusone\"]\npoints = [64]\nmaster_seed = 7\n",
    );
    write(&bench_dir.join("live/arms/plusone/src/lib.rs"), &ARM_LIB.replace("OUTPUT_SIZE", "8"));
    // The living code: a crate of the repository, reached through a support
    // crate and an engine crate that takes it the way hilavitkutin spells its
    // siblings, inherited from the workspace (`plus.workspace = true`).
    write(
        &bench_dir.join("support/glue/Cargo.toml"),
        "[package]\nname = \"glue\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\nengine = { path = \"../../../crates/engine\" }\n",
    );
    write(
        &bench_dir.join("support/glue/src/lib.rs"),
        "pub use engine::f;\n",
    );
    write(
        &mock_dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n\n[workspace.dependencies]\nplus = { path = \"crates/plus\" }\n",
    );
    write(
        &mock_dir.join("crates/engine/Cargo.toml"),
        "[package]\nname = \"engine\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\nplus.workspace = true\n",
    );
    write(&mock_dir.join("crates/engine/src/lib.rs"), "pub use plus::f;\n");
    write(
        &mock_dir.join("crates/plus/Cargo.toml"),
        "[package]\nname = \"plus\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    let original = "pub fn f(x: u64) -> u64 { x + 1 }\n";
    let plus = mock_dir.join("crates/plus/src/lib.rs");
    write(&plus, original);

    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let run = || {
        let code = mockspace::bench::cmd(&cfg, &["run", "live", "sec"]);
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", std::process::ExitCode::SUCCESS),
            "the run"
        );
    };

    run();
    run();
    write(&plus, "pub fn f(x: u64) -> u64 { x.wrapping_add(1) }\n");
    run();
    write(&plus, original);
    run();

    // The arm reaches `plus` only through the workspace `engine` inherits from,
    // so the edit moving its source is the case that fails without cargo being
    // asked; the section's variant, by its own directory, the same.
    let mut first = String::new();
    for bench in ["live", "sec"] {
        let all = runs(&bench_dir, bench);
        assert_eq!(
            all.len(),
            4,
            "{bench}: every run is written, none skipped as already measured: {all:?}"
        );
        let source = |i: usize| all[i].1[0].source.clone();
        let at = source(0);
        assert_eq!(
            at.len(),
            12,
            "{bench}: a tracking row names its arm's source, not a marker: {at}"
        );
        assert_eq!(source(1), at, "{bench}: the same code again stands at the same point");
        assert_ne!(source(2), at, "{bench}: changed living code is a new point");
        assert_eq!(source(3), at, "{bench}: the code put back lands under its earlier point");
        if bench == "live" {
            first = at;
        } else {
            assert_ne!(at, first, "each arm has a source of its own");
        }
    }
    let all = runs(&bench_dir, "live");
    let source = |i: usize| all[i].1[0].source.clone();

    for (dir, rows) in &all {
        let class = dir
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            rows[0].system.starts_with(&class),
            "the row names its system: {} in {class}",
            rows[0].system
        );
    }

    let index = std::fs::read_to_string(bench_dir.join("results/INDEX.md")).unwrap();
    let system = &all[0].1[0].system;
    assert!(
        index.contains(&format!("| {system} | plusone | `{first}` | 3 |")),
        "the first point gathers its three runs:\n{index}"
    );
    assert!(
        index.contains(&format!("| {system} | plusone | `{}` | 1 |", source(2))),
        "{index}"
    );
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
