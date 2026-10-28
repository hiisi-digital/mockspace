//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A field typed `date`, `time`, `datetime` or `span` is a string held to its form: the
//! generated schema carries the form and taplo refuses a row holding one written another
//! way, while the same text in a `string` field passes.

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

/// A project with one namespace whose `when` is of type `ty`, and one row holding `value`
/// in it, as a string.
fn fixture(root: &Path, ty: &str, value: &str) {
    fixture_literal(root, ty, &format!("\"{value}\""));
}

/// As `fixture`, with the row's value written as `literal` in the TOML, quotes or none.
fn fixture_literal(root: &Path, ty: &str, literal: &str) {
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
key = "event"
title = "Events"
description = "Something that happened."

[[registry.namespace.field]]
name = "when"
type = "{ty}"
description = "When it happened, or for how long."
"#
        ),
    );
    write(
        &mock.join("registry/a.toml"),
        &format!("[[event]]\nid = \"first_one\"\nwhen = {literal}\n"),
    );
}

fn run(root: &Path) -> Output {
    common::mockspace()
        .current_dir(root.join("mock"))
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("the binary runs")
}

fn outcome(ty: &str, value: &str) -> (bool, String) {
    assert!(
        taplo_present(),
        "the form is enforced by taplo. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), ty, value);
    let out = run(tmp.path());
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Each type, the values written in its form, and values a person might write that are
/// not in it.
const CASES: &[(&str, &[&str], &[&str])] = &[
    ("date", &["2026-10-28", "2024-02-29"], &[
        "2026-13-01",
        "2026-10-32",
        "28.10.2026",
        "2026-10-28T10:00",
        "26-10-28",
    ]),
    ("time", &["18:49", "00:00", "18:49:05", "18:49:05.25"], &[
        "24:00", "6:30", "18:60", "18:49:61", "18h49",
    ]),
    (
        "datetime",
        &[
            "2026-10-27T18:49:00Z",
            "2026-10-27 18:49",
            "2026-10-27T18:49:00.123Z",
            "2026-10-27T18:49+02:00",
        ],
        &[
            "2026-10-27",
            "2026-10-27T18:49:00ZZ",
            "2026/10/27 18:49",
            "2026-10-27T25:00",
            "18:49",
        ],
    ),
    (
        "span",
        &["P3D", "PT1H30M", "P1W", "P1Y2M3DT4H5M6.5S", "PT0.5S", "P2M"],
        &["P", "PT", "P1DT", "1h", "P1H", "PT1D", "3 days"],
    ),
];

/// The control for the refusals below: every value written in its type's form passes, so
/// a refusal is about the form and not about the fixture.
#[test]
fn a_value_written_in_its_types_form_passes() {
    for (ty, good, _) in CASES {
        for v in *good {
            let (ok, err) = outcome(ty, v);
            assert!(ok, "`{v}` is a {ty}:\n{err}");
        }
    }
}

#[test]
fn a_value_written_another_way_is_refused() {
    for (ty, _, bad) in CASES {
        for v in *bad {
            let (ok, err) = outcome(ty, v);
            assert!(
                !ok && err.contains(v),
                "`{v}` is not a {ty} and must be refused, naming it:\n{err}"
            );
        }
    }
}

/// Without this, the refusals are consistent with a build refusing these texts in any field.
#[test]
fn the_same_text_in_a_string_field_passes() {
    for (_, _, bad) in CASES {
        for v in *bad {
            let (ok, err) = outcome("string", v);
            assert!(ok, "`{v}` is a string:\n{err}");
        }
    }
}

/// The form is checked and the calendar is not, which the reference states as a limit. The
/// second value is the control: a day written in the wrong form is refused, so the first
/// passing is the calendar going unchecked and not the pattern accepting anything.
#[test]
fn a_day_the_calendar_lacks_passes_the_form() {
    let (ok, err) = outcome("date", "2026-02-30");
    assert!(ok, "`2026-02-30` is in the form of a date:\n{err}");
    let (ok, _) = outcome("date", "2026-02-3");
    assert!(!ok, "`2026-02-3` is not in the form of a date");
}

fn outcome_literal(ty: &str, literal: &str) -> (bool, String) {
    assert!(
        taplo_present(),
        "the form is enforced by taplo. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    fixture_literal(tmp.path(), ty, literal);
    let out = run(tmp.path());
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A bare TOML date, time or datetime is not a string. Taplo hands it to the schema as
/// one, so the pattern passes it, and the loader is what refuses it, saying to quote it.
/// The quoted spelling of each, in the same field, is the control.
#[test]
fn a_toml_date_or_time_is_refused_until_it_is_quoted() {
    let cases = [
        ("date", "2026-10-28"),
        ("time", "18:49:05"),
        ("datetime", "2026-10-27T18:49:00Z"),
    ];
    for (ty, text) in cases {
        let (ok, err) = outcome_literal(ty, text);
        assert!(!ok, "bare `{text}` in a {ty} field passed:\n{err}");
        assert!(
            err.contains("unquoted-time") && err.contains("Quote it"),
            "`{text}` is refused without saying to quote it:\n{err}"
        );
        let (ok, err) = outcome_literal(ty, &format!("\"{text}\""));
        assert!(ok, "quoted `{text}` is a {ty}:\n{err}");
    }
}

/// The same bare date in a `string` field is a different matter: the field holds text
/// and nothing says it is a moment.
#[test]
fn a_toml_date_in_a_string_field_is_left_alone() {
    let (ok, err) = outcome_literal("string", "2026-10-28");
    assert!(ok, "{err}");
}
