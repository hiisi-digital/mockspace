//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock registry-append` adds one string to a declared list field of an existing row and
//! prints nothing, refuses what the project does not declare and leaves the file alone when it
//! does, and hands the step to a delegate a workspace placed above the repository, which can
//! take it whole, decline it, or be skipped with `--local`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

const ROWS: &str = "[[ticket]]\nid = \"first\"\ntitle = \"One\"\nanswers = [\"a\"]\n\n[[ticket]]\nid = \"second\"\ntitle = \"Two\"\n";

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

/// A repository `ws/repo` with a ticket namespace declaring a list and a text field.
fn fixture(ws: &Path) -> PathBuf {
    let root = ws.join("repo");
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    write(&mock.join("Cargo.toml"), "[workspace]\nmembers = []\nresolver = \"2\"\n");
    write(
        &mock.join("mockspace.toml"),
        r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work."

[[registry.namespace.field]]
name = "title"
type = "string"
description = "What it is."

[[registry.namespace.field]]
name = "answers"
type = "string[]"
description = "What was said."
"#,
    );
    write(&mock.join("registry/a.toml"), ROWS);
    root
}

fn append(root: &Path, args: &[&str]) -> Output {
    common::mockspace()
        .current_dir(root.join("mock"))
        .env_remove("CARGO_TARGET_DIR")
        .arg("registry-append")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn rows(root: &Path) -> String {
    fs::read_to_string(root.join("mock/registry/a.toml")).unwrap()
}

fn place_hook(dir: &Path, body: &str) {
    let path = dir.join(".mockspace/hooks/registry-append");
    write(&path, &format!("#!/bin/sh\n{body}\n"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_value_is_added_to_the_row_and_nothing_is_printed() {
    let ws = tempfile::tempdir().unwrap();
    let root = fixture(ws.path());
    let out = append(&root, &["ticket::second", "answers", "b"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty());
    assert_eq!(
        rows(&root),
        ROWS.replace("title = \"Two\"\n", "title = \"Two\"\nanswers = [\"b\"]\n")
    );
    let again = append(&root, &["ticket::second", "answers", "b"]);
    assert!(again.status.success());
    assert_eq!(rows(&root).matches("\"b\"").count(), 1, "a second call adds nothing");
}

#[test]
fn what_the_project_does_not_declare_is_refused_and_the_file_stays() {
    let ws = tempfile::tempdir().unwrap();
    let root = fixture(ws.path());
    for args in [
        ["nothing::first", "answers", "x"],
        ["ticket::first", "unknown", "x"],
        ["ticket::first", "title", "x"],
        ["ticket::ghost", "answers", "x"],
    ] {
        let out = append(&root, &args);
        assert!(!out.status.success(), "{args:?} was accepted");
        assert_eq!(rows(&root), ROWS, "{args:?} changed the file");
    }
}

#[test]
fn a_delegate_above_the_repository_takes_the_step_whole_or_declines_it() {
    let ws = tempfile::tempdir().unwrap();
    let root = fixture(ws.path());
    let said = ws.path().join("said");
    place_hook(
        ws.path(),
        &format!(
            "printf '%s|%s|%s' \"$1\" \"$2\" \"$3\" >> {}\n[ \"$3\" = decline ] && exit 3\nexit 0",
            said.display()
        ),
    );

    let whole = append(&root, &["ticket::first", "answers", "words"]);
    assert!(whole.status.success());
    assert_eq!(rows(&root), ROWS, "the delegate took it whole, so mockspace wrote nothing");
    assert_eq!(fs::read_to_string(&said).unwrap(), "ticket::first|answers|words");

    let declined = append(&root, &["ticket::first", "answers", "decline"]);
    assert!(declined.status.success());
    assert!(rows(&root).contains("answers = [\"a\", \"decline\"]"));

    let local = append(&root, &["ticket::first", "answers", "mine", "--local"]);
    assert!(local.status.success());
    assert!(rows(&root).contains("\"mine\""));
    assert_eq!(
        fs::read_to_string(&said).unwrap(),
        "ticket::first|answers|wordsticket::first|answers|decline",
        "--local ran no delegate"
    );
}

#[test]
fn a_delegate_that_fails_fails_the_call_and_the_file_stays() {
    let ws = tempfile::tempdir().unwrap();
    let root = fixture(ws.path());
    place_hook(ws.path(), "echo broken >&2\nexit 9");
    let out = append(&root, &["ticket::first", "answers", "x"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("broken"));
    assert_eq!(rows(&root), ROWS);
}
