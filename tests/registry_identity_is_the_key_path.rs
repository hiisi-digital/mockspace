//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A registry row is its key path, and the files it is written in are no part of it.
//!
//! Any number of `*.toml` files, under any names and in any depth of directory under
//! `registry/`, make one registry. Splitting a file, merging two, renaming one or moving a row to
//! another file changes nothing a reader, a check or a relation sees. Each scenario writes the
//! same rows into a different layout and compares everything the registry answers.
//!
//! The comparison is deliberately wide: the rows and their fields, the order a namespace lists
//! them in, the table cells a rendered page is built from, what refers to what, and every finding
//! the checks report. Anything left out of it is a place a layout could leak through unseen. Where
//! a row lives is the one thing it leaves out, because that is the one answer that is meant to
//! change, and it is only ever a place to look.

use std::fs;
use std::path::Path;

use mockspace::registry::{
    Registry, RegistryNamespace, load_registry, referrers, table_cells, validate,
    validate_row_references,
};

const CONFIG: &str = "\
[[registry.namespace]]
key = \"subject\"
title = \"Subjects\"

[[registry.namespace.field]]
name = \"what\"
type = \"string\"

[[registry.namespace]]
key = \"tenet\"
title = \"Tenets\"

[[registry.namespace.field]]
name = \"subject\"
type = \"subject\"

[[registry.namespace.field]]
name = \"text\"
type = \"string\"
";

/// One row, as its TOML block.
struct Row {
    ns:   &'static str,
    id:   &'static str,
    body: &'static str,
}

impl Row {
    fn block(&self) -> String {
        format!("[[{}]]\nid = \"{}\"\n{}\n", self.ns, self.id, self.body)
    }
}

/// The rows every layout writes. Their order here is deliberately neither the order of their ids
/// nor the order of any file name a layout gives them.
fn rows() -> Vec<Row> {
    vec![
        Row { ns: "subject", id: "time", body: "what = \"when things happen\"" },
        Row { ns: "tenet", id: "the_clock_is_one", body: "subject = \"time\"\ntext = \"one clock\"" },
        Row { ns: "subject", id: "sky", body: "what = \"what hangs above\"" },
        Row { ns: "tenet", id: "a_day_has_a_sun", body: "subject = \"sky\"\ntext = \"a sun\"" },
        Row { ns: "subject", id: "planet", body: "what = \"where it happens\"" },
        Row { ns: "tenet", id: "up_is_local", body: "subject = \"planet\"\ntext = \"up\"" },
        Row { ns: "tenet", id: "nights_follow_days", body: "subject = \"sky\"\ntext = \"nights\"" },
    ]
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Write `rows` as the registry of a fresh mock directory, one `(file, row indexes)` per entry,
/// and load it.
fn load(layout: &[(&str, &[usize])]) -> (tempfile::TempDir, Vec<RegistryNamespace>, Registry) {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    write(&mock.join("mockspace.toml"), CONFIG);
    let all = rows();
    for (file, idx) in layout {
        let text: String = idx.iter().map(|&i| all[i].block()).collect::<Vec<_>>().join("\n");
        write(&mock.join("registry").join(file), &text);
    }
    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = load_registry(mock, &cfg.registry_namespaces);
    (dir, cfg.registry_namespaces, reg)
}

/// Everything the registry answers except where a row was written.
fn answers(namespaces: &[RegistryNamespace], reg: &Registry) -> String {
    let mut out = String::new();
    for (qualified, row) in &reg.rows {
        out.push_str(&format!("{qualified} {:?}\n", row.fields));
    }
    for (ns, ids) in &reg.by_namespace {
        out.push_str(&format!("order {ns}: {}\n", ids.join(" ")));
    }
    for ns in namespaces {
        let (cols, cells) = table_cells(ns, reg);
        out.push_str(&format!("table {}: {cols:?} {cells:?}\n", ns.key));
    }
    for q in reg.rows.keys().filter(|q| q.starts_with("subject::")) {
        out.push_str(&format!("referrers {q}: {:?}\n", referrers(reg, namespaces, q)));
    }
    for f in validate(namespaces, reg)
        .into_iter()
        .chain(validate_row_references(reg, namespaces))
    {
        out.push_str(&format!("finding {}: {}\n", f.kind, f.message));
    }
    out
}

#[test]
fn the_same_rows_in_one_file_in_many_and_in_nested_directories_are_one_registry() {
    let one = load(&[("everything.toml", &[0, 1, 2, 3, 4, 5, 6])]);
    // file names sorting against the ids, rows split across namespaces
    let many = load(&[
        ("z_first.toml", &[0, 5]),
        ("a_last.toml", &[1, 2]),
        ("m.toml", &[3, 4, 6]),
    ]);
    // a different depth for each, one directory named for a subject and one for nothing
    let nested = load(&[
        ("tenet/sky.toml", &[3, 6]),
        ("tenet/time.toml", &[1]),
        ("deep/er/still/planet.toml", &[4, 5]),
        ("subject.toml", &[0, 2]),
    ]);

    let base = answers(&one.1, &one.2);
    assert!(
        base.contains("subject::sky") && base.contains("tenet::up_is_local"),
        "the fixture did not load, so equal answers prove nothing:\n{base}"
    );
    assert_eq!(base, answers(&many.1, &many.2), "many files differ from one");
    assert_eq!(base, answers(&nested.1, &nested.2), "nested files differ from one");
}

#[test]
fn a_namespace_lists_its_rows_by_key_whichever_files_hold_them() {
    let (_d, _ns, reg) = load(&[("a.toml", &[0]), ("b.toml", &[4, 2])]);
    assert_eq!(
        reg.by_namespace["subject"],
        ["subject::planet", "subject::sky", "subject::time"],
        "the order follows the files rather than the key paths"
    );
}

#[test]
fn renaming_or_moving_a_file_changes_nothing() {
    let before = load(&[("tenet/sky.toml", &[1, 3, 6]), ("subject.toml", &[0, 2, 4, 5])]);
    let after = load(&[("notes/whatever.toml", &[1, 3, 6]), ("zz/moved.toml", &[0, 2, 4, 5])]);
    assert_eq!(answers(&before.1, &before.2), answers(&after.1, &after.2));
}

#[test]
fn a_moved_row_keeps_every_relation_pointing_at_it() {
    let at_home = load(&[("tenet.toml", &[1, 3, 5, 6]), ("subject.toml", &[0, 2, 4])]);
    // `sky` leaves the subject file for a file of its own, under a directory
    let moved = load(&[
        ("tenet.toml", &[1, 3, 5, 6]),
        ("subject.toml", &[0, 4]),
        ("sky/sky.toml", &[2]),
    ]);
    let (ns, home, away) = (&at_home.1, &at_home.2, &moved.2);
    let want = ["tenet::a_day_has_a_sun", "tenet::nights_follow_days"];
    assert_eq!(referrers(home, ns, "subject::sky"), want);
    assert_eq!(referrers(away, ns, "subject::sky"), want, "the move cut a relation");
    assert!(validate_row_references(away, ns).is_empty());
}

#[test]
fn one_key_path_in_two_files_is_refused_and_both_files_are_named() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    write(&mock.join("mockspace.toml"), CONFIG);
    let sky = rows()[2].block();
    write(&mock.join("registry/subject.toml"), &sky);
    write(&mock.join("registry/elsewhere/in/the/tree/again.toml"), &sky);
    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = load_registry(mock, &cfg.registry_namespaces);

    let findings = validate(&cfg.registry_namespaces, &reg);
    assert_eq!(findings.len(), 1, "{:?}", findings.iter().map(|f| &f.message).collect::<Vec<_>>());
    let f = &findings[0];
    assert_eq!(f.kind, "duplicate-id");
    assert!(f.message.contains("subject::sky"), "{}", f.message);
    for file in ["registry/subject.toml", "registry/elsewhere/in/the/tree/again.toml"] {
        assert!(f.message.contains(file), "{file} is not named in: {}", f.message);
    }
}

#[test]
fn the_same_row_id_in_two_namespaces_is_two_rows() {
    // a namespace is the first segment of the key path, and ids are only unique within one
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    write(&mock.join("mockspace.toml"), CONFIG);
    write(
        &mock.join("registry/a.toml"),
        "[[subject]]\nid = \"sky\"\nwhat = \"x\"\n\n[[tenet]]\nid = \"sky\"\nsubject = \"sky\"\ntext = \"y\"\n",
    );
    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = load_registry(mock, &cfg.registry_namespaces);
    assert!(reg.duplicates.is_empty(), "{:?}", reg.duplicates);
    assert!(reg.get("subject::sky").is_some() && reg.get("tenet::sky").is_some());
}
