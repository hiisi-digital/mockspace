//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! An executable is written by `mockspace_manifest::write_executable`, and by
//! nothing else, in the engine and in a test alike.
//!
//! A plain write followed by a `chmod` leaves the file open for writing in the
//! window where another thread of the process forks, and Linux then refuses the
//! exec with "Text file busy". The engine's own writers are exposed to it as
//! much as a test is, since a lib test runs them on many threads and then runs
//! what they wrote. The writer in `mockspace-manifest/src/exec.rs` is the one
//! that keeps its descriptor out of any process that forks, so what is checked
//! here is the spelling of the other way: a permission set on a file by
//! `set_permissions`, `from_mode`, `set_mode`, an `OpenOptionsExt` mode or a
//! `chmod` command, in any `.rs` file of the repository bar the writer.
//!
//! The check is a text search and not a parse. It reads a line that names one of
//! the spellings and is not a comment, so a mode set through a spelling this list
//! does not carry is the one thing it cannot see; the list is where such a
//! spelling is added.

use std::path::PathBuf;

/// The scrubs of a child's environment, shared with the lib tests by path.
#[path = "common/ambient.rs"]
mod ambient;
/// The source files of the repository.
#[path = "common/sources.rs"]
mod sources;

/// The spellings that give a file its execute bit, or any mode at all.
const SPELLINGS: &[&str] = &[
    "set_permissions",
    "from_mode",
    "set_mode",
    "OpenOptionsExt",
    "\"chmod\"",
    "chmod ",
];

/// Files that may set a mode, with the reason each is not a second writer.
const EXEMPT: &[(&str, &str)] = &[
    ("mockspace-manifest/src/exec.rs", "it is the writer"),
    (
        "tests/an_executable_is_written_by_the_one_writer.rs",
        "it names the spellings it looks for",
    ),
];

/// Every `file:line` in `text` that sets a mode, for a file at `rel`.
fn violations(rel: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            !line.trim_start().starts_with("//") && SPELLINGS.iter().any(|s| line.contains(s))
        })
        .map(|(i, line)| format!("{rel}:{}: {}", i + 1, line.trim()))
        .collect()
}

/// Every `file:line` in a shared test-support file that names a unix-only path.
/// The support files are compiled into every integration target, so a path that
/// exists on unix alone, ungated, stops all of them compiling elsewhere.
fn unix_only(rel: &str, text: &str) -> Vec<String> {
    if !rel.starts_with("tests/common/") {
        return Vec::new();
    }
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//") && line.contains("os::unix"))
        .map(|(i, line)| format!("{rel}:{}: {}", i + 1, line.trim()))
        .collect()
}

fn repository() -> (PathBuf, Vec<PathBuf>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources::rust_files(&root, &mut files);
    assert!(
        files.len() > 40,
        "the walk found {} files; it is looking in the wrong place",
        files.len()
    );
    (root, files)
}

#[test]
fn nothing_sets_a_mode_except_the_one_writer() {
    let (root, files) = repository();
    let mut found = Vec::new();
    for path in &files {
        let rel = sources::relative(&root, path);
        if EXEMPT.iter().any(|(e, _)| *e == rel) {
            continue;
        }
        found.extend(violations(&rel, &std::fs::read_to_string(path).unwrap()));
    }
    assert!(
        found.is_empty(),
        "these set a mode; write the file with `mockspace_manifest::write_executable` (\
         `crate::test_exec` in the lib tests, `common::write_executable` in the integration \
         tests):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_shared_test_support_names_no_unix_only_path() {
    let (root, files) = repository();
    let support = files
        .iter()
        .filter(|p| sources::relative(&root, p).starts_with("tests/common/"))
        .count();
    assert!(
        support >= 3,
        "the walk found {support} support files; it is looking in the wrong place"
    );
    let mut found = Vec::new();
    for path in &files {
        let rel = sources::relative(&root, path);
        found.extend(unix_only(&rel, &std::fs::read_to_string(path).unwrap()));
    }
    assert!(
        found.is_empty(),
        "shared test support is compiled everywhere; gate this with `cfg(unix)` or move it into \
         `mockspace_manifest`, which does:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_exempt_file_exists_and_still_needs_the_exemption() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (rel, why) in EXEMPT {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel} is exempt ({why}) and cannot be read: {e}"));
        let names_one = SPELLINGS.iter().any(|s| text.contains(s));
        assert!(
            names_one,
            "{rel} is exempt ({why}) but sets no mode; drop the exemption"
        );
    }
}

#[test]
fn a_plain_write_then_chmod_is_flagged_in_test_code_and_in_production_code() {
    let plain = "std::fs::write(&p, body).unwrap();\n\
                 std::fs::set_permissions(&p, Permissions::from_mode(0o755)).unwrap();\n";
    assert_eq!(
        violations("tests/x.rs", plain).len(),
        1,
        "an integration test"
    );
    assert_eq!(
        violations("src/a/b_tests.rs", plain).len(),
        1,
        "a lib test file"
    );
    // the shapes the engine's own writers had
    assert_eq!(violations("src/a/b.rs", plain).len(), 1, "production code");
    assert_eq!(
        violations("mockspace-manifest/src/gate.rs", plain).len(),
        1,
        "a published crate"
    );
    let engine = "let mut perms = meta.permissions();\nperms.set_mode(0o755);\n";
    assert_eq!(violations("src/bootstrap/hooks.rs", engine).len(), 1);
    // another spelling of the same thing
    assert_eq!(
        violations("tests/x.rs", "Command::new(\"chmod\").arg(\"+x\");").len(),
        1
    );
    assert_eq!(
        violations("src/x.rs", "use std::os::unix::fs::OpenOptionsExt;").len(),
        1
    );
}

#[test]
fn a_call_to_the_writer_and_a_comment_are_not_flagged() {
    assert!(violations("src/a/b.rs", "write_executable(&p, body)?;\n").is_empty());
    assert!(violations("tests/x.rs", "common::write_executable(&p, body);\n").is_empty());
    assert!(violations("tests/x.rs", "// set_permissions would race\n").is_empty());
    assert!(violations("src/x.rs", "/// `set_mode` is the writer's alone\n").is_empty());
}

#[test]
fn an_ungated_unix_path_in_the_shared_support_is_flagged_and_elsewhere_is_not() {
    let text = "use std::os::unix::fs::PermissionsExt;\n";
    assert_eq!(unix_only("tests/common/exec.rs", text).len(), 1);
    assert_eq!(unix_only("tests/common/mod.rs", text).len(), 1);
    assert!(
        unix_only("tests/x.rs", text).is_empty(),
        "a single target gates for itself"
    );
    assert!(unix_only("tests/common/exec.rs", "// os::unix is not named here\n").is_empty());
}
