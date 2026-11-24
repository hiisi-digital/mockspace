//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit fixers leave a staged file formatted, whatever `clippy --fix` rewrote in it.
//!
//! `rustfmt` ran first and `clippy --fix` after, so a rewrite clippy made, a collapsed `if let`
//! nest being the one met, was committed as clippy printed it, and `rustfmt --check` failed on the
//! commit the fixers had just made. Found on a commit whose staged file held such a nest.

mod common;

use std::fs;
use std::process::Command;

const NESTED: &str = "pub fn f(o: Option<u8>) -> bool {\n    if let Some(x) = o {\n        if x > 1 {\n            return true;\n        }\n    }\n    false\n}\n";

#[test]
fn a_file_clippy_rewrote_is_left_formatted() {
    let tmp = mockspace_manifest::scratch::tempdir().expect("a temporary directory");
    let root = tmp.path();
    common::git_init(root);
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .expect("a manifest");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(root.join("src/lib.rs"), NESTED).expect("a lib.rs");
    common::git_in_dir(root, &["add", "-A"]);

    mockspace::autofix::run(root, true, true);

    let after = fs::read_to_string(root.join("src/lib.rs")).expect("the file after");
    // the control: clippy rewrote the nest, or the check below says nothing about its rewrite
    assert_ne!(after, NESTED, "clippy --fix changed nothing, so the case did not arise");
    let check = Command::new("rustfmt")
        .args(["--check", "--edition", "2024", "src/lib.rs"])
        .current_dir(root)
        .output()
        .expect("rustfmt runs");
    assert!(
        check.status.success(),
        "the fixers left the file unformatted:\n{after}\n{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
