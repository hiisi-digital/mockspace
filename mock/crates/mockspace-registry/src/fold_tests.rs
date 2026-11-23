//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Folding a namespace's path fields into its ids, and reading them back.

use super::*;
use crate::test_world::{namespace, rows_from};

/// `kind` and `subject` as namespaces of their own, the subjects hierarchical, and `ticket` folding
/// both.
fn world() -> Vec<RegistryNamespace> {
    vec![
        namespace("kind", &[], &[]),
        namespace("subject", &[], &[]),
        namespace(
            "ticket",
            &[("kind", "kind", true), ("subject", "subject", true), ("title", "string", true)],
            &["kind", "subject"],
        ),
    ]
}

const REFERENCED: &str = r#"
[[kind]]
id = "question"

[[kind]]
id = "task"

[[subject]]
id = "engine::internals"

[[subject]]
id = "engine::render"

[[subject]]
id = "tools"
"#;

fn with(tickets: &str, nss: &[RegistryNamespace]) -> Registry {
    rows_from(&format!("{REFERENCED}{tickets}"), nss)
}

fn kinds(found: &[RegistryFinding]) -> Vec<&str> {
    found.iter().map(|f| f.kind).collect()
}

#[test]
fn the_folded_fields_are_read_back_from_the_id() {
    let nss = world();
    let reg = with("[[ticket]]\nid = \"question::engine::internals::deps_allowed\"\ntitle = \"x\"\n", &nss);
    let row = reg.get("ticket::question::engine::internals::deps_allowed").expect("the row loads under its id");
    assert_eq!(row.fields.get("kind").map(String::as_str), Some("question"));
    // The subject is itself a path, found among the subject ids.
    assert_eq!(row.fields.get("subject").map(String::as_str), Some("engine::internals"));
    assert!(reg.folds.is_empty(), "{:?}", reg.folds);
    assert!(validate(&nss, &reg).is_empty(), "{:?}", validate(&nss, &reg));
    // The derived fields are row references like any other, and every one names a row.
    assert!(validate_row_references(&reg, &nss).is_empty());
}

#[test]
fn a_row_writing_a_folded_field_is_reported() {
    let nss = world();
    let bad = with("[[ticket]]\nid = \"task::tools::lint\"\nkind = \"task\"\ntitle = \"x\"\n", &nss);
    let found = validate(&nss, &bad);
    assert_eq!(kinds(&found), ["folded-field-written"], "{found:?}");
    assert!(found[0].message.contains("ticket::task::tools::lint") && found[0].message.contains("`kind`"));
    // The id is the owner: what the row reads is what its id says.
    assert_eq!(bad.get("ticket::task::tools::lint").unwrap().fields["kind"], "task");
    let good = with("[[ticket]]\nid = \"task::tools::lint\"\ntitle = \"x\"\n", &nss);
    assert!(validate(&nss, &good).is_empty());
}

#[test]
fn a_segment_naming_no_row_is_reported() {
    let nss = world();
    for id in ["idea::tools::lint", "task::engine::lint", "task::engine::physics::lint", "task::tools"] {
        let reg = with(&format!("[[ticket]]\nid = \"{id}\"\ntitle = \"x\"\n"), &nss);
        let found = validate(&nss, &reg);
        assert_eq!(kinds(&found), ["unfoldable-id"], "{id}: {found:?}");
        assert!(found[0].message.contains(&format!("ticket::{id}")), "{}", found[0].message);
        assert!(!reg.get(&format!("ticket::{id}")).unwrap().fields.contains_key("kind"));
    }
}

#[test]
fn an_id_that_is_no_path_is_reported() {
    let nss = world();
    let reg = with("[[ticket]]\nid = \"task::Tools::lint\"\ntitle = \"x\"\n", &nss);
    assert_eq!(kinds(&validate(&nss, &reg)), ["unfoldable-id"]);
}

/// `path = ["kind"]` alone, so everything between the kind and the leaf is free.
fn kind_only() -> Vec<RegistryNamespace> {
    vec![
        namespace("kind", &[], &[]),
        namespace("ticket", &[("kind", "kind", true), ("title", "string", true)], &["kind"]),
    ]
}

#[test]
fn free_names_stand_between_the_folded_fields_and_the_leaf() {
    // Op's example: kind `question`, the free names `engine::internals::deps`, the leaf `allowed`.
    let nss = kind_only();
    let ids = ["question::engine::internals::deps::allowed", "question::allowed", "task::a::b"];
    let tickets: String = ids.iter().map(|id| format!("[[ticket]]\nid = \"{id}\"\ntitle = \"x\"\n\n")).collect();
    let reg = with(&tickets, &nss);
    assert!(validate(&nss, &reg).is_empty(), "{:?}", validate(&nss, &reg));
    for (id, kind) in [(ids[0], "question"), (ids[1], "question"), (ids[2], "task")] {
        let row = reg.get(&format!("ticket::{id}")).unwrap();
        assert_eq!(row.fields["kind"], kind, "{id}");
        assert_eq!(row.fields.keys().collect::<Vec<_>>(), ["id", "kind", "title"], "free names give no field");
    }
    // With a subject folded too, the free names come after it.
    let nss = world();
    let reg = with("[[ticket]]\nid = \"question::engine::internals::deps::allowed\"\ntitle = \"x\"\n", &nss);
    assert!(validate(&nss, &reg).is_empty(), "{:?}", validate(&nss, &reg));
    assert_eq!(reg.get("ticket::question::engine::internals::deps::allowed").unwrap().fields["subject"], "engine::internals");
}

#[test]
fn a_folded_field_with_no_leaf_after_it_does_not_read() {
    let nss = kind_only();
    let reg = with("[[ticket]]\nid = \"question\"\ntitle = \"x\"\n", &nss);
    assert_eq!(kinds(&validate(&nss, &reg)), ["unfoldable-id"]);
    let reg = with("[[ticket]]\nid = \"idea::engine::a\"\ntitle = \"x\"\n", &nss);
    assert_eq!(kinds(&validate(&nss, &reg)), ["unfoldable-id"], "a first name naming no kind");
}

#[test]
fn an_optional_folded_field_is_refused_at_the_declaration() {
    // Free names may follow it, so `question::tools::x` would read with a subject `tools` or
    // without one and `tools` free.
    let nss = vec![
        namespace("kind", &[], &[]),
        namespace("subject", &[], &[]),
        namespace("ticket", &[("kind", "kind", true), ("subject", "subject", false)], &["kind", "subject"]),
    ];
    let found = unfit_path_fields(&nss);
    assert_eq!(kinds(&found), ["unfit-path-field"]);
    assert!(found[0].message.contains("optional") && found[0].message.contains("free names"), "{}", found[0].message);
    assert!(unfit_path_fields(&world()).is_empty());
}

#[test]
fn an_id_reading_two_ways_is_reported_with_both_readings() {
    // `engine::core::tools::a` is area `engine` and subject `core`, or area `engine::core` and
    // subject `tools`, the rest free in both.
    let nss = vec![
        namespace("area", &[], &[]),
        namespace("subject", &[], &[]),
        namespace("ticket", &[("area", "area", true), ("subject", "subject", true)], &["area", "subject"]),
    ];
    let rows = "[[area]]\nid = \"engine\"\n\n[[area]]\nid = \"engine::core\"\n\n[[subject]]\nid = \"core\"\n\n\
                [[subject]]\nid = \"tools\"\n\n[[ticket]]\nid = \"engine::core::tools::a\"\n";
    let reg = rows_from(rows, &nss);
    let found: Vec<_> = validate(&nss, &reg).into_iter().filter(|f| f.kind == "unfoldable-id").collect();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("area = engine, subject = core"), "{}", found[0].message);
    assert!(found[0].message.contains("area = engine::core, subject = tools"), "{}", found[0].message);
    // Where the data leaves one reading, the same declaration folds.
    let rows = "[[area]]\nid = \"engine\"\n\n[[subject]]\nid = \"tools\"\n\n[[ticket]]\nid = \"engine::tools::a\"\n";
    assert!(validate(&nss, &rows_from(rows, &nss)).is_empty());
}

#[test]
fn fold_row_id_is_a_pure_function_of_the_declaration_and_the_ids() {
    let nss = world();
    let referenced: BTreeMap<String, BTreeSet<String>> = [
        ("kind", vec!["question"]),
        ("subject", vec!["engine::internals", "engine::render"]),
        ("ticket", vec![]),
    ]
    .into_iter()
    .map(|(k, ids)| (k.to_string(), ids.into_iter().map(String::from).collect()))
    .collect();
    let declared: BTreeSet<&str> = referenced.keys().map(String::as_str).collect();
    let ticket = &nss[2];
    assert_eq!(
        fold_row_id(ticket, &declared, &referenced, "question::engine::render::free::x").unwrap(),
        [("kind".to_string(), "question".to_string()), ("subject".to_string(), "engine::render".to_string())]
    );
    assert_eq!(fold_row_id(ticket, &declared, &referenced, "question::x"), Err(IdFault::NoReading), "the subject is required");
    assert_eq!(fold_row_id(ticket, &declared, &referenced, "question::engine::x"), Err(IdFault::NoReading));
    assert!(matches!(fold_row_id(ticket, &declared, &referenced, "question::"), Err(IdFault::NotAnId(_))));
    // A namespace folding nothing reads no values from any id.
    assert_eq!(fold_row_id(&nss[0], &declared, &referenced, "anything::at::all").unwrap(), []);
    // The written-field check is pure too.
    assert_eq!(folded_fields_written(ticket, ["title", "kind", "subject", "note"]), ["kind", "subject"]);
    assert!(folded_fields_written(ticket, ["title"]).is_empty());
}

#[test]
fn a_path_naming_a_field_that_cannot_fold_is_reported_once() {
    let cases: [(&[(&str, &str, bool)], &[&str]); 7] = [
        (&[("kind", "kind", true)], &["missing"]),
        (&[("kind", "kind[]", true)], &["kind"]),
        (&[("kind", "string", true)], &["kind"]),
        (&[("kind", "kind", true)], &["kind", "kind"]),
        (&[("kind", "ticket", true)], &["kind"]),
        (&[("kind", "nothing", true)], &["kind"]),
        (&[("kind", "kind", false)], &["kind"]),
    ];
    for (fields, path) in cases {
        let nss = vec![namespace("kind", &[], &[]), namespace("ticket", fields, path)];
        let found = unfit_path_fields(&nss);
        assert_eq!(kinds(&found), ["unfit-path-field"], "{path:?} {fields:?}");
        // The rows of such a namespace are not reported one by one for it.
        let reg = rows_from("[[kind]]\nid = \"a\"\n\n[[ticket]]\nid = \"a::b\"\n", &nss);
        assert!(reg.folds.is_empty(), "{:?}", reg.folds);
    }
    assert!(unfit_path_fields(&world()).is_empty());
}

#[test]
fn a_load_of_one_namespace_reads_the_namespaces_its_ids_fold() {
    let nss = world();
    let text = format!("{REFERENCED}[[ticket]]\nid = \"task::tools::lint\"\ntitle = \"x\"\n");
    let wanted: BTreeSet<String> = ["ticket".to_string()].into();
    let reg = crate::load::rows_of_texts(Path::new("reg"), vec![("t.toml".into(), Some(text))], &nss, Some(&wanted));
    assert!(reg.folds.is_empty(), "{:?}", reg.folds);
    assert_eq!(reg.get("ticket::task::tools::lint").unwrap().fields["subject"], "tools");
}

#[test]
fn the_schema_does_not_require_a_folded_field_and_takes_a_path_id() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    let mock = dir.join("mock");
    fs::create_dir_all(&mock).unwrap();
    crate::generate_schemas(&dir, &mock, &world());
    let schema = fs::read_dir(crate::schemas_dir(&dir))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .find(|t| t.contains("\"ticket\""))
        .expect("a schema naming ticket");
    assert!(schema.contains(r#""required": ["id", "title"]"#), "{schema}");
    assert!(schema.contains(r#"(::[a-z][a-z0-9_]*)*$"#), "{schema}");
}
