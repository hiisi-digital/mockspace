//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::path::Path;

use super::edit::append;
use super::hook::{self, Taken};
use super::parse;

const FILE: &str = "\
# the questions

# which era comes first
[[ticket]]
id = \"first\"
title = \"One\"
answers = [\"2026-10-01: a\"]

[[ticket]]
id = \"second\"
title = \"Two\"
answers = [
  \"2026-10-02: b\",
  \"2026-10-03: c\", # kept
]

[[ticket]]
id = \"third\"
title = \"Three\"
";

fn text_after(id: &str, value: &str) -> String {
    append(FILE, "ticket", id, "answers", value)
        .unwrap()
        .expect("the value was new")
}

#[test]
fn a_value_joins_a_one_line_list_and_nothing_else_moves() {
    let after = text_after("first", "2026-10-04: d");
    assert!(after.contains("answers = [\"2026-10-01: a\", \"2026-10-04: d\"]"));
    assert_eq!(
        after.replace(", \"2026-10-04: d\"", ""),
        FILE,
        "every other byte of the file is as it was"
    );
}

#[test]
fn a_value_joins_a_spread_list_on_a_line_of_its_own_with_the_comment_kept() {
    let after = text_after("second", "2026-10-04: d");
    assert!(after.contains("  \"2026-10-03: c\", # kept\n  \"2026-10-04: d\",\n]"), "{after}");
}

#[test]
fn a_row_without_the_field_gets_a_list_of_one() {
    let after = text_after("third", "2026-10-04: d");
    assert!(after.contains("title = \"Three\"\nanswers = [\"2026-10-04: d\"]"), "{after}");
    assert!(after.starts_with(&FILE[.. FILE.find("id = \"third\"").unwrap()]));
}

#[test]
fn a_value_the_field_holds_is_not_added_again() {
    assert_eq!(
        append(FILE, "ticket", "first", "answers", "2026-10-01: a").unwrap(),
        None
    );
}

#[test]
fn a_row_that_is_not_in_the_file_is_refused() {
    assert!(append(FILE, "ticket", "ghost", "answers", "x").is_err());
    assert!(append(FILE, "other", "first", "answers", "x").is_err());
}

#[test]
fn a_field_that_is_not_a_list_is_refused() {
    let err = append(FILE, "ticket", "first", "title", "x").unwrap_err();
    assert!(err.contains("other than a list"), "{err}");
}

#[test]
fn the_arguments_are_a_target_a_field_and_a_value() {
    let a = parse(&["ticket::first", "answers", "v"]).unwrap();
    assert_eq!((a.ns, a.id, a.field, a.value, a.local), ("ticket", "first", "answers", "v", false));
    assert!(parse(&["--local", "ticket::first", "answers", "v"]).unwrap().local);
    assert!(parse(&["ticket", "answers", "v"]).is_err());
    assert!(parse(&["ticket::", "answers", "v"]).is_err());
    assert!(parse(&["ticket::first", "answers"]).is_err());
}

fn place_hook(root: &Path, body: &str) -> std::path::PathBuf {
    let dir = root.join(".mockspace/hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("registry-append");
    crate::test_exec::write_executable(&path, format!("#!/bin/sh\n{body}\n"));
    path
}

#[test]
fn the_nearest_hook_above_a_repository_is_the_one_found() {
    let ws = tempfile::tempdir().unwrap();
    let repo = ws.path().join("repo");
    let deeper = repo.join("sub");
    std::fs::create_dir_all(&deeper).unwrap();
    assert_eq!(hook::find(&deeper), None);
    let outer = place_hook(ws.path(), "exit 0");
    assert_eq!(hook::find(&deeper), Some(outer));
    let inner = place_hook(&repo, "exit 0");
    assert_eq!(hook::find(&deeper), Some(inner));
}

#[test]
fn a_hook_is_told_the_step_and_its_status_is_the_answer() {
    let ws = tempfile::tempdir().unwrap();
    let said = ws.path().join("said");
    let hook_path = place_hook(
        ws.path(),
        &format!(
            "printf '%s|%s|%s|%s' \"$1\" \"$2\" \"$3\" \"$MOCKSPACE_MOCK_DIR\" > {}\n[ \"$3\" = decline ] && exit 3\n[ \"$3\" = break ] && {{ echo no >&2; exit 9; }}\nexit 0",
            said.display()
        ),
    );
    let mock = ws.path().join("mock");
    let go = |v: &str| hook::run(&hook_path, ws.path(), &mock, "ticket::first", "answers", v);
    assert_eq!(go("words"), Ok(Taken::Whole));
    assert_eq!(
        std::fs::read_to_string(&said).unwrap(),
        format!("ticket::first|answers|words|{}", mock.display())
    );
    assert_eq!(go("decline"), Ok(Taken::Declined));
    let err = go("break").unwrap_err();
    assert!(err.contains("status 9") && err.contains("no"), "{err}");
}
