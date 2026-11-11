//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The shell both gating hooks share, run as it is by default: no `MOCKSPACE_VERBOSE`.
//!
//! A stub `cargo` stands in for the launcher. It prints progress on a run, appends one finding to
//! the findings file when told the run is red, and answers `gate-report` with a fixed summary, so
//! what is under test is `ms_gate`'s own wiring: what reaches the terminal, the exit status, and
//! the temporary files. The summary itself is `gate_report`'s, tested there.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use super::hooks::gate_shell;

const STUB: &str = r#"#!/usr/bin/env bash
if [ "$2" = "gate-report" ]; then
    [ -n "${STUB_REPORT_FAILS:-}" ] && exit 3
    echo "blocked: $3"
    echo "  findings: $(wc -l < "$5" | tr -d ' ')"
    exit 0
fi
echo "Compiling noise"
echo "progress line" >&2
if [ "${STUB_RED:-}" = 1 ]; then
    printf 'error\tfile-size\ta.rs:1\tmessage\n' >> "$MOCKSPACE_GATE_FINDINGS"
    exit 1
fi
exit 0
"#;

/// Run `ms_gate pre-commit --lint-only --commit` with the stub; returns the output and the files
/// the run left in its temporary directory.
fn run(env: &[(&str, &str)]) -> (Output, Vec<String>) {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let scratch = tmp.path().join("tmp");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&scratch).unwrap();
    write_stub(&bin.join("cargo"));
    let script = format!("{}\nms_gate pre-commit --lint-only --commit\n", gate_shell());
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(script)
        .env_remove("MOCKSPACE_VERBOSE")
        .env("TMPDIR", &scratch)
        .env("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let left = fs::read_dir(&scratch)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    (out, left)
}

fn write_stub(path: &Path) {
    crate::test_exec::write_executable(path, STUB);
}

#[test]
fn a_green_gate_prints_nothing_and_leaves_no_temporary_file() {
    let (out, left) = run(&[]);
    assert!(out.status.success(), "{out:?}");
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_red_gate_exits_non_zero_and_prints_the_report_summary_alone() {
    let (out, left) = run(&[("STUB_RED", "1")]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "blocked: pre-commit\n  findings: 1\n",
        "neither the progress lines nor the raw output reach the terminal"
    );
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_report_that_fails_still_blocks_and_shows_the_raw_output() {
    let (out, left) = run(&[("STUB_RED", "1"), ("STUB_REPORT_FAILS", "1")]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Compiling noise") && err.contains("progress line"), "{err}");
    assert!(err.ends_with("blocked: pre-commit\n"), "{err}");
    assert!(left.is_empty(), "left behind: {left:?}");
}
