//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock bench disasm <bench> <arm> <symbol>`, run start to finish on a
//! nested tree with one manifest-less arm: the arm is built as a run builds
//! it, the named function's disassembly lands as a results artifact in the
//! layout, a second call lands beside the first, and a function the arm does
//! not have fails and writes nothing.

#[path = "common/ambient.rs"]
mod ambient;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

const ARM_LIB: &str = r#"
#[unsafe(no_mangle)]
pub extern "C" fn plusone_kernel(x: u64) -> u64 {
    x.wrapping_mul(31).wrapping_add(7)
}
"#;

#[test]
fn a_named_function_of_an_arm_is_written_as_a_results_artifact() {
    let repo = env!("CARGO_MANIFEST_DIR");
    let repo_toml = mockspace_manifest::toml_text::basic_string(repo);
    let root_guard = mockspace_manifest::scratch::tempdir().unwrap();
    let mock_dir = root_guard.path().join("mock");
    let bench_dir = mock_dir.join("benches");
    write(
        &bench_dir.join("bench.toml"),
        &format!("\n[build]\nmockspace = '{{ path = {repo_toml} }}'\n"),
    );
    write(
        &bench_dir.join("hash").join("bench.toml"),
        "\ntitle = \"Plus one\"\nworkload = \"default\"\narms = [\"plusone\"]\npoints = [64]\n",
    );
    write(&bench_dir.join("hash/arms/plusone/src/lib.rs"), ARM_LIB);
    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let ok = format!("{:?}", ExitCode::SUCCESS);
    let results = bench_dir.join("results");

    let code = mockspace::bench::cmd(&cfg, &["disasm", "hash", "plusone", "plusone_kernel"]);
    assert_eq!(format!("{code:?}"), ok);
    let written = files_under(&results);
    assert_eq!(written.len(), 1, "{written:?}");
    let rel = written[0].strip_prefix(&bench_dir).unwrap();
    assert_eq!(
        mockspace_bench_harness::layout::place(rel, |_| true),
        mockspace_bench_harness::layout::Placement::InLayout,
        "{}",
        rel.display()
    );
    assert!(rel.starts_with("results/hash/disasm"), "{}", rel.display());
    assert!(
        rel.ends_with("plusone/plusone_kernel.txt"),
        "{}",
        rel.display()
    );
    let text = std::fs::read_to_string(&written[0]).unwrap();
    assert!(
        text.contains("`plusone_kernel` of arm `plusone` of bench `hash`"),
        "{text}"
    );
    let body: Vec<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    assert!(
        body[0].trim_end().ends_with("plusone_kernel>:")
            || body[0].trim_end().ends_with("plusone_kernel:"),
        "{text}"
    );
    assert!(
        body.len() >= 2,
        "the label and at least one instruction: {text}"
    );
    assert!(
        mockspace_bench_harness::layout::runs(&results).is_empty(),
        "an artifact is no run"
    );

    // a second artifact lands beside the first
    let code = mockspace::bench::cmd(&cfg, &["disasm", "hash", "plusone", "plusone_kernel"]);
    assert_eq!(format!("{code:?}"), ok);
    let again = files_under(&results);
    assert_eq!(again.len(), 2, "{again:?}");
    assert_eq!(
        std::fs::read_to_string(&written[0]).unwrap(),
        text,
        "the first stands untouched"
    );

    // a function the arm does not have fails and writes nothing
    let code = mockspace::bench::cmd(&cfg, &["disasm", "hash", "plusone", "no_such_fn"]);
    assert_ne!(format!("{code:?}"), ok);
    assert_eq!(files_under(&results), again);

    // an arm the bench does not have fails before anything is built
    let code = mockspace::bench::cmd(&cfg, &["disasm", "hash", "nobody", "plusone_kernel"]);
    assert_ne!(format!("{code:?}"), ok);
}
