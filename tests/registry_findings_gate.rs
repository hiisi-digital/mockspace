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

#[test]
fn a_clean_registry_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), false);
    assert_eq!(
        run(tmp.path()),
        Some(0),
        "the control: without this passing, the test above is equally consistent \
         with a binary that fails on every registry"
    );
}
