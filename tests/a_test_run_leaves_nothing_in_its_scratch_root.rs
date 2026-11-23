//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------
//! Every test binary of this checkout, run whole under a temp dir of its own, leaves nothing under
//! the scratch directory: no test dir, no process root.

#[path = "common/ambient.rs"]
mod ambient;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Builds (or finds already built, by the same cargo and target dir this run uses) the lib test
/// binary and the named integration test binaries of this checkout, and returns their paths.
fn binaries_of_this_checkout(integration: &[&str]) -> Vec<(String, PathBuf)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["test", "--no-run", "--message-format=json", "--lib"]);
    for t in integration {
        cmd.args(["--test", t]);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["profile"]["test"] == true)
        .filter_map(|v| {
            let exe = v["executable"].as_str()?.to_string();
            let name = v["target"]["name"].as_str()?.to_string();
            Some((name, PathBuf::from(exe)))
        })
        .collect()
}

/// Every path under `dir`, process roots included.
fn leftovers(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            found.push(e.path());
            if e.path().is_dir() {
                todo.push(e.path());
            }
        }
    }
    found
}

#[test]
fn a_test_run_leaves_nothing_in_its_scratch_root() {
    // Two integration tests that make scratch, a fast one and the slowest direct user: the rest
    // spawn cargo and run for minutes, too long here.
    let wanted = ["bench_generated_e2e", "help_for_a_name"];
    let found = binaries_of_this_checkout(&wanted);
    let mut names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    let mut expect: Vec<&str> = wanted.to_vec();
    expect.push("mockspace");
    expect.sort();
    assert_eq!(names, expect, "the lib binary and each named test binary were found");

    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    for (name, exe) in &found {
        let out = Command::new(exe).env("TMPDIR", tmp.path()).output().unwrap();
        let tail = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{name}: {}",
            tail.lines().rev().take(15).collect::<Vec<_>>().join("\n")
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("test result: ok"),
            "{name} ran its tests"
        );
    }
    let base = tmp.path().join(mockspace_manifest::scratch::SCRATCH_DIR);
    let left = leftovers(&base);
    assert!(left.is_empty(), "scratch left behind: {left:?}");
    // The control: a path made by hand and not dropped is seen.
    fs::create_dir_all(base.join("1/kept")).unwrap();
    assert_eq!(leftovers(&base).len(), 2);
}
