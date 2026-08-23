//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A registry finding reported as an ERROR must fail the command.
//!
//! The registry design says duplicate identifier and dangling reference are
//! errors "because both mean the registry is lying". Until 2026-08-22 the code
//! agreed in words and not in status: every finding printed `ERROR [kind]` and
//! the command returned `ExitCode::SUCCESS`, so a lying registry was
//! indistinguishable from a clean one to anything downstream. A gate that
//! reports and does not gate is the failure this whole mechanism exists to
//! prevent, committed by the mechanism itself.
//!
//! Two arms over one fixture shape, differing only in whether the registry
//! lies. **The clean arm is the control and it is not decoration**: without it,
//! "the broken arm exits non-zero" is equally consistent with a binary that
//! always exits non-zero, which is exactly what an earlier version of this
//! fixture did for unrelated reasons.

use std::fs;
use std::path::Path;
use std::process::Command;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repo whose registry either declares one slug twice or does not.
///
/// The manifest is memberless on purpose: it is the shape mockspace's own
/// cargo gate forgives, so the fixture fails for the reason under test rather
/// than for a missing workspace.
fn fixture(root: &Path, duplicated: bool) {
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
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."

[[registry.namespace.field]]
name = "question"
type = "string"
required = true
description = "The question the spike answers."
"#,
    );
    // Slugs, not prefixed numbers: the schema generator emits
    // `^[a-z][a-z0-9_]*$`, so a row written the way the older design document
    // still describes fails validation before this test's subject is reached.
    write(
        &mock.join("registry/a.toml"),
        "[[spike]]\nid = \"first_one\"\nquestion = \"a\"\n",
    );
    if duplicated {
        write(
            &mock.join("registry/b.toml"),
            "[[spike]]\nid = \"first_one\"\nquestion = \"the same slug again\"\n",
        );
    }
}

fn run(root: &Path) -> Option<i32> {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .output()
        .ok()?
        .status
        .code()
}

/// The same run with `taplo` removed from `PATH` and nothing else touched.
///
/// Emptying `PATH` outright was the first attempt and it was wrong: the run
/// shells out to `cargo` too, so the binary panicked before reaching the schema
/// check and both arms failed together. The control caught it. Filtering only
/// the directories that actually hold `taplo` removes the one tool under test
/// and leaves every other lookup working.
fn run_without_taplo(root: &Path) -> Option<i32> {
    let path = std::env::var("PATH").unwrap_or_default();
    let kept: Vec<&str> = path
        .split(':')
        .filter(|d| !d.is_empty() && !Path::new(d).join("taplo").exists())
        .collect();
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .env("PATH", kept.join(":"))
        .output()
        .ok()?
        .status
        .code()
}

#[test]
fn a_duplicate_slug_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), true);
    assert_eq!(
        run(tmp.path()),
        Some(1),
        "a slug declared in two files means a reference to it cannot resolve, \
         and the command must say so in its exit status rather than only in its output"
    );
}

/// Whether `taplo` resolves on this host.
///
/// The clean arm needs it. Without it the schema check cannot run, which is
/// itself a finding now, so a "clean" registry legitimately exits 1 and the
/// control goes red for a reason that has nothing to do with duplicates. That
/// happened: this module's own schema-unavailable gate broke this module's
/// load-bearing control, and it was invisible on a host that has taplo.
///
/// Stated as a precondition rather than skipped, because a control that quietly
/// does not run is the thing the docstring above says must not happen.
fn taplo_present() -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|d| !d.is_empty() && Path::new(d).join("taplo").exists())
}

#[test]
fn a_clean_registry_does_not() {
    assert!(
        taplo_present(),
        "this control cannot discriminate without taplo: the schema check would \
         be unavailable, which now fails the command, so the arm would be red for \
         a reason unrelated to duplicate slugs. Install taplo to run this suite."
    );
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), false);
    assert_eq!(
        run(tmp.path()),
        Some(0),
        "the control: without this passing, the test above is equally consistent \
         with a binary that fails on every registry"
    );
}

/// A run that could not check row shape is not a run that passed.
///
/// The schema check shells out to `taplo`. When it is absent the check cannot
/// run, and until 2026-08-22 that printed `SKIPPED` and exited 0: every
/// required field, every type and every slug pattern went unverified and the
/// command reported success. That is the two-valued-outcome problem in the one
/// place it is most dangerous, because the registry looks checked.
///
/// Gating applies only where namespaces are declared, which is the only place
/// row shape exists to verify. The control below is the same tree with the same
/// empty `PATH` and no namespace, and it must still pass.
#[test]
fn an_unverifiable_schema_check_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), false);
    assert_eq!(
        run_without_taplo(tmp.path()),
        Some(1),
        "with no taplo on PATH the row shape is unchecked, and an unchecked \
         registry must not report success"
    );
}

#[test]
fn a_project_with_no_registry_does_not_need_the_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    // No `[[registry.namespace]]`: nothing here has a row shape to verify.
    write(
        &root.join("mock/mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n",
    );
    assert_eq!(
        run_without_taplo(root),
        Some(0),
        "the control: gating an absent tool must not reach a project that \
         declares no namespaces, or every consumer pays for a feature it does not use"
    );
}

/// The case that separates the rows guard from a namespace guard.
///
/// `a_project_with_no_registry_does_not_need_the_tool` has neither namespaces
/// nor rows, so it passes on either reading and names the wrong one. This
/// declares a namespace and writes no rows: under a namespace guard it would
/// fail, under the rows guard it must not. Verified by hand when the guard was
/// written, and committed so nobody does that again.
#[test]
fn namespaces_with_no_rows_have_no_shape_to_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );
    write(
        &root.join("mock/mockspace.toml"),
        r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."
"#,
    );
    // Declared, and deliberately empty: no registry directory at all.
    assert_eq!(
        run_without_taplo(root),
        Some(0),
        "a namespace with no rows has nothing whose shape could be unverified, \
         so the absent tool must not gate it"
    );
}

/// A reference that resolves to nothing fails the command.
///
/// The design puts dangling reference and duplicate identifier on one rung,
/// both errors, both "because both mean the registry is lying". The first
/// version of this work gated duplicates and left this as a `warning:`, so half
/// the sentence was enforced while the failure summary claimed both. A reviewer
/// found it by reading the design the summary quoted.
///
/// The control is the same document with the reference pointing at a row that
/// exists. Without it, this passes for any binary that fails on every document.
#[test]
fn a_reference_to_no_row_fails_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root, false);
    write(
        &root.join("mock/PROJECT.md.tmpl"),
        "# fixture\n\nSee {{ reg::spike::no_such_row }} for the answer.\n",
    );
    assert_eq!(
        run(root),
        Some(1),
        "a reference resolving to nothing is rendered literally and must fail, \
         because it looks checked and is not"
    );
}

#[test]
fn a_reference_to_a_row_that_exists_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root, false);
    write(
        &root.join("mock/PROJECT.md.tmpl"),
        "# fixture\n\nSee {{ reg::spike::first_one }} for the answer.\n",
    );
    assert_eq!(
        run(root),
        Some(0),
        "the control: a reference that resolves must not be reported, or the \
         arm above passes for a binary that fails on every document"
    );
}

/// An unsubstituted placeholder is not a dangling reference.
///
/// Both survive as literal braces in the output, and only one is a lie about
/// the registry. `{{ reg::spike::nope }}` names a row that does not exist;
/// `{{abi_count}}` is a template placeholder that resolved to nothing because
/// the project has nothing to count.
///
/// The first version of this gate treated them alike and failed a real repo
/// whose own config says a project with no packages is ordinary, over fifteen
/// placeholders in one document. Caught by running against a consumer rather
/// than a fixture, which is why this arm exists.
#[test]
fn an_unsubstituted_placeholder_does_not_fail_the_command() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root, false);
    write(
        &root.join("mock/PROJECT.md.tmpl"),
        "# fixture\n\nThere are {{abi_count}} of them, in {{architecture_table}}.\n",
    );
    assert_eq!(
        run(root),
        Some(0),
        "a placeholder with nothing to substitute is an ordinary state and must \
         not be reported as the registry lying"
    );
}

/// The discriminating pair: same document, one of each.
#[test]
fn a_reference_beside_a_placeholder_still_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root, false);
    write(
        &root.join("mock/PROJECT.md.tmpl"),
        "# fixture\n\n{{abi_count}} of them, see {{ reg::spike::no_such_row }}.\n",
    );
    assert_eq!(
        run(root),
        Some(1),
        "the reference must still gate when a placeholder sits beside it, or the \
         distinction has been drawn by ignoring both"
    );
}
