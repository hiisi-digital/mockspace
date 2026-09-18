//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The switch that turns a commit fixer off is the clone's, so the same key in
//! the machine's global git config does nothing.
//!
//! A file of its own, holding one test, because it sets `GIT_CONFIG_GLOBAL` on
//! the process: the loader runs git with whatever environment it inherits, and
//! a test binary runs its tests on several threads, so a second test in this
//! binary would read the variable too.

use std::fs;
use std::process::Command;

use mockspace::config::Config;

#[test]
fn a_setting_in_the_global_config_is_not_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("clone");
    fs::create_dir_all(root.join("mock")).expect("create mock dir");
    fs::write(
        root.join("mockspace.toml"),
        "project_name = \"probe\"\nmock_dir = \"mock\"\n",
    )
    .expect("write config");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git runs")
            .success()
    );

    let global = tmp.path().join("global.gitconfig");
    fs::write(
        &global,
        "[mockspace]\n\tautoClippyFix = false\n\tautoFmt = false\n",
    )
    .expect("write global");
    // SAFETY: the only test in this binary, so no other thread reads the
    // environment while it changes.
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    }

    // The positive control first: git itself reads the global file from inside
    // the clone, so the loader not reading it is a choice rather than a file
    // nothing could see.
    let seen = Command::new("git")
        .args(["config", "--type=bool", "--get", "mockspace.autoClippyFix"])
        .current_dir(&root)
        .output()
        .expect("git runs");
    assert_eq!(
        String::from_utf8_lossy(&seen.stdout).trim(),
        "false",
        "git did not read the planted global file, so the arm below proves nothing"
    );

    let cfg = Config::from_dir(&root.join("mock"));
    assert!(
        cfg.auto_clippy_fix && cfg.auto_fmt,
        "a global setting reached the clone"
    );
}
