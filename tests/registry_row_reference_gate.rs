//! The row-reference checks, through the binary rather than through the
//! functions.
//!
//! The four checks had 22 unit tests and no test that any of them was ever
//! called. Deleting the whole block from `dispatch.rs` kept the suite green,
//! which is the definition of an unwired gate, and this module is what makes
//! that impossible.
//!
//! Every arm asserts on the exit status **and** on the finding kind in the
//! output. Status alone cannot tell one finding from another, and a fixture
//! that fails for an unrelated reason produces the same 1.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repo declaring `slot` and `answer`, where `answer.slot` references a slot.
///
/// `config` and `rows` are the two halves a caller varies. The manifest is
/// memberless because that is the shape the cargo gate forgives, so a fixture
/// fails for the reason under test rather than for a missing workspace.
fn fixture(root: &Path, config: &str, rows: &str) {
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(
        &mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(&mock.join("mockspace.toml"), config);
    write(&mock.join("registry/data.toml"), rows);
}

fn run(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .output()
        .unwrap()
}

/// Whether `taplo` resolves on this host.
///
/// The clean arm needs it: without it the schema check cannot run, which is
/// itself a finding, so a clean registry legitimately exits 1 and the control
/// goes red for a reason unrelated to row references.
fn taplo_present() -> bool {
    Command::new("taplo")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

const TWO_NAMESPACES: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "slot"
title = "Slots"
description = "Something a person needs."

[[registry.namespace.field]]
name = "use"
type = "string"
required = true
description = "What the person is doing."

[[registry.namespace]]
key = "answer"
title = "Answers"
description = "What answers a slot."

[[registry.namespace.field]]
name = "slot"
type = "slot"
required = true
description = "The slot this answers."
"#;

const GOOD_ROWS: &str = r#"[[slot]]
id = "display"
use = "getting pixels on a screen"

[[answer]]
id = "niri"
slot = "display"
"#;

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// The control, and it is not decoration: without it, every arm below is
/// equally consistent with a binary that always exits 1.
#[test]
fn a_registry_whose_row_references_resolve_passes() {
    if !taplo_present() {
        eprintln!("skipping: taplo is not installed, so a clean registry cannot exit 0");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), TWO_NAMESPACES, GOOD_ROWS);
    let out = run(tmp.path());
    assert_eq!(
        out.status.code(),
        Some(0),
        "a registry with a resolving row reference must pass: {}",
        stderr(&out)
    );
}

#[test]
fn a_slug_naming_no_row_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        TWO_NAMESPACES,
        &GOOD_ROWS.replace("slot = \"display\"", "slot = \"nosuch\""),
    );
    let out = run(tmp.path());
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("unknown-row-reference"),
        "the run failed, but not for this reason: {}",
        stderr(&out)
    );
}

#[test]
fn a_qualified_value_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        TWO_NAMESPACES,
        &GOOD_ROWS.replace("slot = \"display\"", "slot = \"slot::display\""),
    );
    let out = run(tmp.path());
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("malformed-row-reference"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_type_naming_no_namespace_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        &TWO_NAMESPACES.replace("type = \"slot\"", "type = \"slott\""),
        GOOD_ROWS,
    );
    let out = run(tmp.path());
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("unknown-field-type"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_namespace_shadowing_a_builtin_type_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        &TWO_NAMESPACES.replace("key = \"slot\"\n", "key = \"boolean\"\n"),
        GOOD_ROWS,
    );
    let out = run(tmp.path());
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("namespace-shadows-type"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_reference_into_a_value_namespace_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        &TWO_NAMESPACES.replace(
            "description = \"Something a person needs.\"",
            "description = \"Something a person needs.\"\nvalue_field = \"use\"",
        ),
        GOOD_ROWS,
    );
    let out = run(tmp.path());
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("row-reference-to-a-value-namespace"),
        "{}",
        stderr(&out)
    );
}

/// A bad declaration in one namespace must not hide a bad slug in another.
///
/// A project-wide gate stood between the two checks, on the reasoning that a
/// field whose type names no namespace cannot hold a checkable reference. That
/// is true of the field and was applied to the whole run, so fixing a typo in
/// one place produced a second round of failures somewhere the author had not
/// been looking. The gate is gone and this is what keeps it gone.
#[test]
fn a_bad_declaration_does_not_suppress_a_bad_slug_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let config = format!(
        "{TWO_NAMESPACES}\n[[registry.namespace]]\nkey = \"other\"\ntitle = \"Other\"\n\
         description = \"An unrelated namespace with a typo in a field type.\"\n\n\
         [[registry.namespace.field]]\nname = \"slot\"\ntype = \"slott\"\n"
    );
    fixture(
        tmp.path(),
        &config,
        &GOOD_ROWS.replace("slot = \"display\"", "slot = \"nosuch\""),
    );
    let out = run(tmp.path());
    let err = stderr(&out);
    assert!(
        err.contains("unknown-field-type"),
        "the typo must be reported: {err}"
    );
    assert!(
        err.contains("unknown-row-reference"),
        "and the unrelated bad slug must be reported in the same run: {err}"
    );
}

/// The view a lint and a tool are handed, built from a real directory.
///
/// The unit tests build a `Registry` in memory; this one goes through the
/// loader, which is the path the engine actually takes, so a view that is
/// correct over a hand-built fixture and empty over a real one is caught here.
///
/// **What this does not reach**: the two lines that assign the view into
/// `RepoContext` and `ToolContext`. Exercising those needs a compiled tool or
/// lint cdylib in a temporary project, and the fixture for that did not come up
/// in the time available. The assignment is two lines and is visible in the
/// diff, and it is unverified by a test.
#[test]
fn the_view_built_from_a_real_directory_carries_the_edges() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(
        tmp.path(),
        TWO_NAMESPACES,
        &format!("{GOOD_ROWS}\n[[slot]]\nid = \"audio\"\nuse = \"sound\"\n"),
    );
    let mock = tmp.path().join("mock");
    let cfg = mockspace::config::Config::from_dir(&mock);
    let reg = mockspace::registry::load_registry(&cfg.mock_dir, &cfg.registry_namespaces);
    let view = mockspace::registry::build_view(&reg, &cfg.registry_namespaces);

    assert_eq!(view.len(), 3, "two slots and one answer");
    assert_eq!(view.rows_in("slot"), ["slot::audio", "slot::display"]);
    assert_eq!(view.referrers("slot::display"), ["answer::niri"]);
    assert!(
        view.referrers("slot::audio").is_empty(),
        "the slot nothing answers is the finding, and it is an empty list"
    );
    assert_eq!(view.field("answer::niri", "slot"), Some("display"));
}
