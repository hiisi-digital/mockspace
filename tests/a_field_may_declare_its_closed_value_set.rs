//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A field's closed value set reaches the generated schema, and taplo enforces it.
//!
//! A closed set stated only in a field's `description` is not a contract. One
//! project shipped four of them that way and a typo in any passed every gate:
//! the row loaded, the schema check passed, the document rendered, and a lint
//! keyed on the field ignored the value it did not recognise, so the defect that
//! lint existed to catch became invisible rather than louder.
//!
//! Checked through the schema rather than duplicated in Rust, per
//! `FINDING_KINDS`' own reasoning: everything a JSON Schema can express is left
//! to the schema, because two implementations of one contract drift. `enum` is
//! expressible, and `check_schemas` reports `Unavailable` rather than passing
//! when taplo is absent, so the contract does not quietly lapse.

use std::fs;
use std::path::Path;
use std::process::Output;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn taplo_present() -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|d| !d.is_empty() && Path::new(d).join("taplo").exists())
}

/// A project with one namespace whose `rung` is a closed set of two.
fn fixture(root: &Path, rung: &str) {
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(
        &mock.join("mockspace.toml"),
        r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "claim"
title = "Claims"
description = "Something the project says."

[[registry.namespace.field]]
name = "rung"
type = "string"
required = true
values = ["stated", "ratified"]
description = "How firmly this is held."
"#,
    );
    write(
        &mock.join("registry/a.toml"),
        &format!("[[claim]]\nid = \"first_one\"\nrung = \"{rung}\"\n"),
    );
}

fn run(root: &Path) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("the binary runs")
}

/// The control, and the one that makes the arm below mean anything: a value
/// that is in the set passes, so a rejection is about membership rather than
/// about the fixture.
#[test]
fn a_declared_value_passes() {
    assert!(
        taplo_present(),
        "the enum is enforced by taplo, so without it the schema check reports \
         unavailable and every arm here fails for that reason instead. Install \
         taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), "ratified");
    let out = run(tmp.path());
    assert!(
        out.status.success(),
        "`ratified` is one of the two declared values:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_value_outside_the_closed_set_is_refused() {
    assert!(
        taplo_present(),
        "the enum is enforced by taplo. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    // The failure this exists for is a typo, not an invented member.
    fixture(tmp.path(), "ratifed");
    let out = run(tmp.path());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a typo in a closed set must not pass. It is the case where every gate \
         below reports clean and every consumer keyed on the field ignores the \
         value:\n{err}"
    );
    assert!(
        err.contains("ratifed") || err.to_lowercase().contains("enum"),
        "and the finding must name what it rejected:\n{err}"
    );
}

/// A field that declares nothing stays free text, which is the default and the
/// common case. Without this, the two arms above are consistent with a build
/// that rejects any value it was not told about.
#[test]
fn a_field_declaring_no_set_takes_anything() {
    assert!(
        taplo_present(),
        "compared against the two arms above, so it runs under the same \
         conditions. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root, "stated");
    let mock = root.join("mock");
    write(
        &mock.join("mockspace.toml"),
        &fs::read_to_string(mock.join("mockspace.toml"))
            .unwrap()
            .replace("values = [\"stated\", \"ratified\"]\n", ""),
    );
    write(
        &mock.join("registry/a.toml"),
        "[[claim]]\nid = \"first_one\"\nrung = \"anything at all\"\n",
    );
    let out = run(root);
    assert!(
        out.status.success(),
        "a field with no declared set is free text:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
