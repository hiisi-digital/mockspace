//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A tracking bench's source points, end to end through `mock bench run`.
//!
//! The arm reaches a crate outside the bench tree through a support crate.
//! Four runs: one, the same again, one after that crate's source changed, and
//! one after it was put back, as a run at an older revision is. Every run is
//! written; the two at unchanged code stand at one source, the changed code
//! at another, and the run on the code put back lands under the first point
//! again. Each row names the system that ran it.

use std::path::{Path, PathBuf};

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

const ARM_LIB: &str = r#"
use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(input_ptr: *const u8, output_ptr: *mut u8, _n: usize) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { *output = input.wrapping_add(1); }
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
    8
}
"#;

/// Every run directory of the cell, oldest first, with its summary rows.
fn runs(bench_dir: &Path) -> Vec<(PathBuf, Vec<mockspace_bench_harness::history::HistoryEntry>)> {
    mockspace_bench_harness::layout::runs(&bench_dir.join("results"))
        .into_iter()
        .filter(|r| r.bench == "live")
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

    write(
        &bench_dir.join("bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{repo}\" }}'\nopt-level = 0\nlto = \"off\"\ncodegen-units = 16\n\n\
             [timing]\npasses = 1\nruns_per_pass = 20\nbatch_size = 5\nharness_runs = 1\ncooldowns_ms = [0]\n"
        ),
    );
    write(
        &bench_dir.join("live/bench.toml"),
        "kind = \"tracking\"\ntitle = \"Plus one\"\nworkload = \"default\"\narms = [\"plusone\"]\npoints = [64]\nmaster_seed = 7\n",
    );
    write(&bench_dir.join("live/arms/plusone/src/lib.rs"), ARM_LIB);
    // the living code: a crate of the repository, reached through a support crate
    write(
        &bench_dir.join("support/glue/Cargo.toml"),
        "[package]\nname = \"glue\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\nplus = { path = \"../../../crates/plus\" }\n",
    );
    write(
        &bench_dir.join("support/glue/src/lib.rs"),
        "pub use plus::f;\n",
    );
    write(
        &mock_dir.join("crates/plus/Cargo.toml"),
        "[package]\nname = \"plus\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    let original = "pub fn f(x: u64) -> u64 { x + 1 }\n";
    let plus = mock_dir.join("crates/plus/src/lib.rs");
    write(&plus, original);

    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let run = || {
        let code = mockspace::bench::cmd(&cfg, &["run", "live"]);
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

    let all = runs(&bench_dir);
    assert_eq!(
        all.len(),
        4,
        "every run is written, none skipped as already measured: {all:?}"
    );
    let source = |i: usize| all[i].1[0].source.clone();
    let first = source(0);
    assert_eq!(
        first.len(),
        12,
        "a tracking row names its arm's source, not a marker: {first}"
    );
    assert_eq!(
        source(1),
        first,
        "the same code again stands at the same point"
    );
    assert_ne!(source(2), first, "changed living code is a new point");
    assert_eq!(
        source(3),
        first,
        "the code put back lands under its earlier point"
    );

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
