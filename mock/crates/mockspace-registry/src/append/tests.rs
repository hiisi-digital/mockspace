//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use crate::RegistryEnv as _;
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
    assert!(
        after.contains("  \"2026-10-03: c\", # kept\n  \"2026-10-04: d\",\n]"),
        "{after}"
    );
}

#[test]
fn a_row_without_the_field_gets_a_list_of_one() {
    let after = text_after("third", "2026-10-04: d");
    assert!(
        after.contains("title = \"Three\"\nanswers = [\"2026-10-04: d\"]"),
        "{after}"
    );
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
    assert_eq!(
        (a.ns, a.id, a.field, a.value, a.local),
        ("ticket", "first", "answers", "v", false)
    );
    assert!(
        parse(&["--local", "ticket::first", "answers", "v"])
            .unwrap()
            .local
    );
    assert!(parse(&["ticket", "answers", "v"]).is_err());
    assert!(parse(&["ticket::", "answers", "v"]).is_err());
    assert!(parse(&["ticket::first", "answers"]).is_err());
}

fn place_hook(root: &Path, body: &str) -> std::path::PathBuf {
    let dir = root.join(".mockspace/hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("registry-append");
    mockspace_manifest::write_executable(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
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

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work to do."

[[registry.namespace.field]]
name = "title"
type = "string"
required = true
description = "What."

[[registry.namespace.field]]
name = "answers"
type = "string[]"
description = "What was said."
"#;

const ROW: &str = "[[ticket]]\nid = \"one\"\ntitle = \"One\"\n";

/// A project with the config above; `ticket/` is kept on a ref when `mounted`, else in the tree.
fn project(remote: &mockspace_store::test_support::Remote, mounted: bool) -> crate::Env {
    let root = remote.clone_dir("p");
    let decl = if mounted { "\n[[registry.store]]\ndir = \"ticket\"\n" } else { "" };
    std::fs::write(root.join("mock/mockspace.toml"), format!("{CONFIG}{decl}")).unwrap();
    let cfg = crate::Env::from_dir(&root.join("mock")).unwrap();
    crate::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.namespaces);
    cfg
}

#[test]
fn an_append_to_a_row_on_a_ref_goes_through_the_writer_and_skips_a_delegate() {
    let remote = mockspace_store::test_support::Remote::new();
    let cfg = project(&remote, true);
    let stores = cfg.registry_stores().unwrap();
    crate::RegistryWriter::new(&cfg, &stores)
        .write(
            &[("ticket/t.toml".into(), Some(ROW.as_bytes().to_vec()))],
            "seed",
        )
        .unwrap();
    // a delegate that takes the step whole and writes nowhere: were it asked, the answer would be lost
    let marker = cfg.repo_root.join("asked");
    place_hook(
        &cfg.repo_root,
        &format!("touch {}\nexit 0", marker.display()),
    );
    super::step(&cfg, &["ticket::one", "answers", "said it"]).unwrap();
    assert!(
        !marker.exists(),
        "the delegate edits the tree, so it is not asked about a row kept on a ref"
    );
    let held = remote
        .file("refs/heads/mock/registry/ticket", "t.toml")
        .unwrap();
    assert!(held.contains("said it"), "{held}");
}

#[test]
fn an_append_in_the_tree_asks_the_delegate_and_without_one_writes_the_file() {
    let remote = mockspace_store::test_support::Remote::new();
    let cfg = project(&remote, false);
    let file = cfg.mock_dir.join("registry/ticket/t.toml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, ROW).unwrap();
    super::step(&cfg, &["ticket::one", "answers", "plain"]).unwrap();
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("answers = [\"plain\"]")
    );
    let marker = cfg.repo_root.join("asked");
    place_hook(
        &cfg.repo_root,
        &format!("touch {}\nexit 0", marker.display()),
    );
    super::step(&cfg, &["ticket::one", "answers", "by hook"]).unwrap();
    assert!(
        marker.exists(),
        "the delegate is asked about a row in the tree"
    );
    assert!(
        !std::fs::read_to_string(&file).unwrap().contains("by hook"),
        "and it took the step whole"
    );
}
