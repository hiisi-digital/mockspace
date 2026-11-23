//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Folding a namespace's path fields into its ids, and reading them back.

use super::*;
use crate::test_world::{namespace, rows_from};

/// `kind` and `subject` as namespaces of their own, the subjects hierarchical, and `ticket` folding
/// both, `subject` as `optional` says.
fn world(optional: bool) -> Vec<RegistryNamespace> {
    vec![
        namespace("kind", &[], &[]),
        namespace("subject", &[], &[]),
        namespace(
            "ticket",
            &[("kind", "kind", true), ("subject", "subject", !optional), ("title", "string", true)],
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
    let nss = world(false);
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
    let nss = world(false);
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
    let nss = world(false);
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
    let nss = world(false);
    let reg = with("[[ticket]]\nid = \"task::Tools::lint\"\ntitle = \"x\"\n", &nss);
    assert_eq!(kinds(&validate(&nss, &reg)), ["unfoldable-id"]);
}

#[test]
fn an_optional_folded_field_may_be_absent_and_is_no_segment() {
    let nss = world(true);
    let reg = with(
        "[[ticket]]\nid = \"question::loose\"\ntitle = \"x\"\n\n[[ticket]]\nid = \"question::tools::tight\"\ntitle = \"y\"\n",
        &nss,
    );
    assert!(validate(&nss, &reg).is_empty(), "{:?}", validate(&nss, &reg));
    let loose = reg.get("ticket::question::loose").unwrap();
    assert_eq!(loose.fields.get("kind").map(String::as_str), Some("question"));
    assert!(!loose.fields.contains_key("subject"));
    assert_eq!(reg.get("ticket::question::tools::tight").unwrap().fields["subject"], "tools");
    // A required one may not be absent.
    let strict = world(false);
    let reg = with("[[ticket]]\nid = \"question::loose\"\ntitle = \"x\"\n", &strict);
    assert_eq!(kinds(&validate(&strict, &reg)), ["unfoldable-id"]);
}

#[test]
fn an_id_reading_two_ways_is_reported_with_both_readings() {
    // Two optional fields over namespaces that share a name: `engine::a` is an area or a subject.
    let nss = vec![
        namespace("area", &[], &[]),
        namespace("subject", &[], &[]),
        namespace("ticket", &[("area", "area", false), ("subject", "subject", false)], &["area", "subject"]),
    ];
    let rows = "[[area]]\nid = \"engine\"\n\n[[subject]]\nid = \"engine\"\n\n[[ticket]]\nid = \"engine::a\"\n";
    let reg = rows_from(rows, &nss);
    let found = validate(&nss, &reg);
    assert_eq!(kinds(&found), ["unfoldable-id"], "{found:?}");
    assert!(found[0].message.contains("area = engine") && found[0].message.contains("subject = engine"));
    // Where the data leaves one reading, the same declaration folds.
    let rows = "[[area]]\nid = \"engine\"\n\n[[subject]]\nid = \"tools\"\n\n[[ticket]]\nid = \"engine::tools::a\"\n";
    assert!(validate(&nss, &rows_from(rows, &nss)).is_empty());
}

#[test]
fn fold_row_id_is_a_pure_function_of_the_declaration_and_the_ids() {
    let nss = world(true);
    let referenced: BTreeMap<String, BTreeSet<String>> = [
        ("kind", vec!["question"]),
        ("subject", vec!["engine::internals", "engine::render"]),
        ("ticket", vec![]),
    ]
    .into_iter()
    .map(|(k, ids)| (k.to_string(), ids.into_iter().map(String::from).collect()))
    .collect();
    let ticket = &nss[2];
    assert_eq!(
        fold_row_id(ticket, &referenced, "question::engine::render::x").unwrap(),
        [("kind".to_string(), "question".to_string()), ("subject".to_string(), "engine::render".to_string())]
    );
    assert_eq!(fold_row_id(ticket, &referenced, "question::x").unwrap(), [("kind".to_string(), "question".to_string())]);
    assert_eq!(fold_row_id(ticket, &referenced, "question::engine::x"), Err(IdFault::NoReading));
    assert!(matches!(fold_row_id(ticket, &referenced, "question::"), Err(IdFault::NotAnId(_))));
    // A namespace folding nothing reads no values from any id.
    assert_eq!(fold_row_id(&nss[0], &referenced, "anything::at::all").unwrap(), []);
}

#[test]
fn a_path_naming_a_field_that_cannot_fold_is_reported_once() {
    let cases: [(&[(&str, &str, bool)], &[&str]); 5] = [
        (&[("kind", "kind", true)], &["missing"]),
        (&[("kind", "kind[]", true)], &["kind"]),
        (&[("kind", "string", true)], &["kind"]),
        (&[("kind", "kind", true)], &["kind", "kind"]),
        (&[("kind", "ticket", true)], &["kind"]),
    ];
    for (fields, path) in cases {
        let nss = vec![namespace("kind", &[], &[]), namespace("ticket", fields, path)];
        let found = unfit_path_fields(&nss);
        assert_eq!(kinds(&found), ["unfit-path-field"], "{path:?} {fields:?}");
        // The rows of such a namespace are not reported one by one for it.
        let reg = rows_from("[[kind]]\nid = \"a\"\n\n[[ticket]]\nid = \"a::b\"\n", &nss);
        assert!(reg.folds.is_empty(), "{:?}", reg.folds);
    }
    assert!(unfit_path_fields(&world(true)).is_empty());
}

#[test]
fn a_load_of_one_namespace_reads_the_namespaces_its_ids_fold() {
    let nss = world(false);
    let text = format!("{REFERENCED}[[ticket]]\nid = \"task::tools::lint\"\ntitle = \"x\"\n");
    let wanted: BTreeSet<String> = ["ticket".to_string()].into();
    let reg = crate::load::rows_of_texts(Path::new("reg"), vec![("t.toml".into(), Some(text))], &nss, Some(&wanted));
    assert!(reg.folds.is_empty(), "{:?}", reg.folds);
    assert_eq!(reg.get("ticket::task::tools::lint").unwrap().fields["subject"], "tools");
}

#[test]
fn the_schema_does_not_require_a_folded_field_and_takes_a_path_id() {
    let dir = std::env::temp_dir().join(format!("fold-schema-{}", std::process::id()));
    let mock = dir.join("mock");
    fs::create_dir_all(&mock).unwrap();
    crate::generate_schemas(&dir, &mock, &world(false));
    let schema = fs::read_dir(crate::schemas_dir(&dir))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .find(|t| t.contains("\"ticket\""))
        .expect("a schema naming ticket");
    let _ = fs::remove_dir_all(&dir);
    assert!(schema.contains(r#""required": ["id", "title"]"#), "{schema}");
    assert!(schema.contains(r#"(::[a-z][a-z0-9_]*)*$"#), "{schema}");
}
