//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! An integer field's `min` and `max` reach the generated schema, and taplo enforces
//! them, as it enforces a closed value set, which holds values of the field's own type; a
//! bound or a listed value declared where it cannot hold is refused at the declaration.

mod common;

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

/// A project with one namespace whose `weight` is declared as `field`, and one row
/// holding `value` in it.
fn fixture(root: &Path, field: &str, value: &str) {
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
key = "claim"
title = "Claims"
description = "Something the project says."

[[registry.namespace.field]]
name = "weight"
{field}
description = "How much it weighs."
"#
        ),
    );
    write(
        &mock.join("registry/a.toml"),
        &format!("[[claim]]\nid = \"first_one\"\nweight = {value}\n"),
    );
}

fn run(root: &Path) -> Output {
    common::mockspace()
        .current_dir(root.join("mock"))
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("the binary runs")
}

const RANGED: &str = "type = \"integer\"\nmin = 1\nmax = 5";

fn outcome(field: &str, value: &str) -> (bool, String) {
    assert!(
        taplo_present(),
        "the range is enforced by taplo. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), field, value);
    let out = run(tmp.path());
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The control: a value at either end of the range passes, so a rejection below is
/// about the range rather than about the fixture.
#[test]
fn a_value_at_either_end_passes() {
    for v in ["1", "5"] {
        let (ok, err) = outcome(RANGED, v);
        assert!(ok, "{v} is inside 1 to 5:\n{err}");
    }
}

#[test]
fn a_value_one_past_either_end_is_refused_for_the_range() {
    for (v, cause) in [
        ("0", "0 is less than the minimum of 1"),
        ("6", "6 is greater than the maximum of 5"),
    ] {
        let (ok, err) = outcome(RANGED, v);
        assert!(
            !ok && err.contains(cause),
            "{v} is outside 1 to 5 and must be refused for it:\n{err}"
        );
    }
}

/// A closed set on an integer or a boolean holds values of that type, so a row holding a
/// listed one passes and one holding another is refused for not being one of them.
#[test]
fn a_closed_set_of_integers_or_booleans_holds_its_own_type() {
    let ints = "type = \"integer\"\nvalues = [\"1\", \"2\"]";
    for v in ["1", "2"] {
        let (ok, err) = outcome(ints, v);
        assert!(ok, "{v} is listed:\n{err}");
    }
    let (ok, err) = outcome(ints, "3");
    assert!(
        !ok && err.contains("3 is not one of [1,2]"),
        "3 is not listed:\n{err}"
    );
    let (ok, err) = outcome("type = \"boolean\"\nvalues = [\"true\"]", "true");
    assert!(ok, "true is listed:\n{err}");
    let (ok, err) = outcome("type = \"boolean\"\nvalues = [\"true\"]", "false");
    assert!(
        !ok && err.contains("false is not one of [true]"),
        "false is not listed:\n{err}"
    );
}

#[test]
fn a_listed_value_no_row_can_hold_is_refused_at_the_declaration() {
    for field in [
        "type = \"integer\"\nvalues = [\"one\"]",
        "type = \"integer\"\nmin = 1\nmax = 5\nvalues = [\"9\"]",
        "type = \"boolean\"\nvalues = [\"yes\"]",
        "type = \"integer\"\nvalues = [\"+5\"]",
        "type = \"integer\"\nvalues = [\"05\"]",
    ] {
        let (ok, err) = outcome(field, "1");
        assert!(!ok && err.contains("unfit-field-value"), "{field}:\n{err}");
    }
}

/// Without this, the arm above is consistent with a build refusing every integer.
#[test]
fn an_integer_declaring_no_bound_takes_anything() {
    let (ok, err) = outcome("type = \"integer\"", "99");
    assert!(ok, "an unbounded integer takes 99:\n{err}");
}

#[test]
fn a_bound_on_a_string_or_a_min_above_its_max_is_refused() {
    let (ok, err) = outcome("type = \"string\"\nmax = 5", "\"heavy\"");
    assert!(
        !ok && err.contains("misplaced-field-bound"),
        "a bound on a string:\n{err}"
    );
    let (ok, err) = outcome("type = \"integer\"\nmin = 5\nmax = 1", "3");
    assert!(
        !ok && err.contains("misplaced-field-bound"),
        "min above max:\n{err}"
    );
}
