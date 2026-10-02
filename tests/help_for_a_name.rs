//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock help <name>` outside any project.
//!
//! Help resolves before project discovery so that it works where there is no
//! project, which is where a stranger first types it. `mock help <name>` lets
//! a non-builtin name through to discovery so a project tool can be described
//! from its pack, and that must not turn a plain help request outside a
//! project into "no mockspace.toml found".

use std::process::{Command, Output};

fn engine_in(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the binary runs")
}

#[test]
fn help_for_an_unknown_name_outside_a_project_prints_the_general_help() {
    let tmp = tempfile::tempdir().unwrap();
    let out = engine_in(tmp.path(), &["help", "nosuchthing"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(stdout.contains("SUBCOMMANDS"), "{stdout}\n{stderr}");
    assert!(!stderr.contains("no mockspace.toml"), "{stderr}");
}

#[test]
fn help_for_a_builtin_outside_a_project_describes_that_builtin() {
    // The control: a builtin is answered in full with no project, and not by
    // the general help, so the arm above is not passing by printing the
    // general help for every name.
    let tmp = tempfile::tempdir().unwrap();
    let out = engine_in(tmp.path(), &["help", "lock"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout.contains("mock lock"), "{stdout}");
    assert!(!stdout.contains("SUBCOMMANDS"), "{stdout}");
}
