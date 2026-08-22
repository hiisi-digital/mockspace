//! The registry gate runs on the path the pre-commit hook actually invokes.
//!
//! Every registry validator lived on the generation path. The generated
//! pre-commit hook ends in `--lint-only --commit --scope <...>`, which skips
//! generation, so until 2026-08-22 a registry that was lying committed without
//! a word: a missing required field, a duplicate slug and a dangling row
//! reference all passed. Worse than passing, the repo lints that read the
//! registry ran against the lying data and reported on it as though it were
//! sound.
//!
//! Found by hand: a required `rung` was deleted from one row and the commit
//! gate said `all repo lints passed`. The same tree, regenerated, printed
//! `ERROR [schema]: "rung" is a required property`. This is that hand check,
//! kept.
//!
//! Three arms over one fixture. **The clean arm is the control and carries the
//! test**: without it, two failing arms are equally consistent with a binary
//! that fails on this path for any reason at all, which is what the first
//! version of this fixture did (the memberless manifest, since fixed).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// What the fixture's registry gets wrong, if anything.
enum Lie {
    /// Nothing. The control.
    None,
    /// A required field left out of one row.
    MissingRequired,
    /// One slug declared in two files, so a reference to it cannot resolve.
    DuplicateSlug,
    /// A typed row reference naming a row that does not exist.
    DanglingReference,
}

fn fixture(root: &Path, lie: &Lie) {
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

[[registry.namespace.field]]
name = "follows"
type = "spike"
required = false
description = "The spike this one continues, as a row reference."
"#,
    );
    let first = match lie {
        // The required field, gone. Everything else about the row is well
        // formed, so the run fails for the reason under test.
        Lie::MissingRequired => "[[spike]]\nid = \"first_one\"\n".to_string(),
        Lie::DanglingReference => {
            "[[spike]]\nid = \"first_one\"\nquestion = \"a\"\nfollows = \"no_such_spike\"\n"
                .to_string()
        },
        _ => "[[spike]]\nid = \"first_one\"\nquestion = \"a\"\n".to_string(),
    };
    write(&mock.join("registry/a.toml"), &first);
    if matches!(lie, Lie::DuplicateSlug) {
        write(
            &mock.join("registry/b.toml"),
            "[[spike]]\nid = \"first_one\"\nquestion = \"the same slug again\"\n",
        );
    }
}

/// The commit gate, spelled exactly as the generated pre-commit hook spells it.
///
/// `--scope infra` rather than a crate list because the fixture has no crates,
/// and because that is the arm an infrastructure-only commit takes: a registry
/// edit on its own stages no crate file, so this is the path a registry change
/// is gated on and the one that was running nothing.
fn commit_gate(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .current_dir(root.join("mock"))
        .args(["--lint-only", "--commit", "--scope", "infra"])
        .output()
        .expect("the binary runs")
}

/// Whether `taplo` resolves on this host.
///
/// The required-field arm is taplo's finding, so without it that arm cannot
/// distinguish its subject from the schema-unavailable finding, which fires on
/// every arm including the control. Stated as a precondition rather than
/// skipped silently.
fn taplo_present() -> bool {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|d| !d.is_empty() && Path::new(d).join("taplo").exists())
}

/// The control, and the one that makes the other three mean anything.
#[test]
fn a_sound_registry_passes_the_commit_gate() {
    if !taplo_present() {
        eprintln!("taplo absent: the schema check reports unavailable, which fails every arm");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), &Lie::None);
    let out = commit_gate(tmp.path());
    assert!(
        out.status.success(),
        "a registry with nothing wrong with it must pass the gate, or the three \
         failing arms below establish nothing about registries:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_missing_required_field_fails_the_commit_gate() {
    if !taplo_present() {
        eprintln!("taplo absent: required fields are checked by taplo, so this arm cannot run");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), &Lie::MissingRequired);
    let out = commit_gate(tmp.path());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a row missing a field its namespace declares required is a row the \
         schema refuses, and the commit gate is where that has to be said:\n{err}"
    );
    assert!(
        err.contains("registry check failed"),
        "the gate must fail for the registry rather than incidentally, so the \
         reason is named in the output:\n{err}"
    );
}

#[test]
fn a_duplicate_slug_fails_the_commit_gate() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), &Lie::DuplicateSlug);
    let out = commit_gate(tmp.path());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "one slug in two files means a reference to it cannot resolve:\n{err}"
    );
    assert!(
        err.contains("duplicate-id"),
        "the finding kind must reach the output, so the author is told what to \
         fix rather than that something is wrong:\n{err}"
    );
}

#[test]
fn a_dangling_row_reference_fails_the_commit_gate() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), &Lie::DanglingReference);
    let out = commit_gate(tmp.path());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a typed field naming a row that does not exist is the case typed row \
         references were introduced to make impossible:\n{err}"
    );
    assert!(
        err.contains("no_such_spike"),
        "the offending slug must appear, or the author has to find it:\n{err}"
    );
}
