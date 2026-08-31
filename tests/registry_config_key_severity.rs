//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `[lints.registry-config-keys]` governs an inert config key, the way any
//! other lint's severity is configured, rather than the warning being a fixed
//! fact of the binary.
//!
//! `config_unknown_keys` never went through the workspace's general lint
//! dispatch, so nothing a project wrote in `[lints]` reached it: the warning
//! printed on every run, forever, with no config-side way to silence it short
//! of removing the key it complains about. This pins the escape hatch this
//! repository chose, `registry-config-keys` as an ordinary lint name, plus the
//! opposite direction: a project may also raise it to a blocking error.

use std::fs;
use std::path::Path;
use std::process::Command;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A registry namespace carrying `prefix`, a key mockspace does not read.
/// `lints_section` is spliced in verbatim, so a variant with no override at
/// all is the empty string.
fn fixture(root: &Path, lints_section: &str) {
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(
        &mock.join("mockspace.toml"),
        &format!(
            r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."
prefix = "SPK"

{lints_section}
"#
        ),
    );
    write(&mock.join("registry/a.toml"), "");
}

fn run(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .env(
            "PATH",
            std::env::var("PATH")
                .unwrap_or_default()
                .split(':')
                .filter(|d| !d.is_empty() && !Path::new(d).join("taplo").exists())
                .collect::<Vec<_>>()
                .join(":"),
        )
        .output()
        .expect("mockspace runs")
}

/// The control: with no `[lints]` override, `prefix` is reported and never blocks.
#[test]
fn the_default_is_a_warning_that_does_not_block() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), "");
    let out = run(tmp.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown-config-key") && stderr.contains("prefix"),
        "the default arm must still report the inert key: {stderr}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "a warning must not fail the command: {stderr}"
    );
}

#[test]
fn silencing_it_removes_the_warning_and_still_does_not_block() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        "[lints.registry-config-keys]\nseverity = \"off\"\n",
    );
    let out = run(tmp.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unknown-config-key"),
        "severity = \"off\" must silence the finding entirely: {stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{stderr}");
}

/// A project may also decide the opposite: an inert key is a defect worth
/// blocking on, not merely a note. Default invocation runs in build mode, so
/// a plain `severity = "error"` (hard-error on every gate) is enough; a lower
/// bar (`build = "error"` alone) would prove the same thing and is not needed
/// to make the point.
#[test]
fn escalating_it_blocks_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        "[lints.registry-config-keys]\nseverity = \"error\"\n",
    );
    let out = run(tmp.path());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ERROR") && stderr.contains("unknown-config-key"),
        "{stderr}"
    );
    assert_eq!(out.status.code(), Some(1), "{stderr}");
}
