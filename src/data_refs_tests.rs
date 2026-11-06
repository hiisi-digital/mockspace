use super::*;

fn decl(ns: &str, paths: &[&str]) -> DataRefDecl {
    DataRefDecl {
        namespace: Some(ns.into()),
        paths:     paths.iter().map(|p| (*p).to_string()).collect(),
        ..DataRefDecl::default()
    }
}

#[test]
fn a_declaration_with_a_namespace_and_paths_is_accepted() {
    assert_eq!(validate(&[decl("registry", &["registry"]), decl("rest", &["research", "benches"])]), Ok(()));
}

#[test]
fn nothing_declared_is_accepted() {
    assert_eq!(validate(&[]), Ok(()));
}

#[test]
fn every_mistake_is_named_in_one_run() {
    let bad = DataRefDecl::default();
    let why = validate(&[bad, decl("a", &["x", "x/y"]), decl("a", &["/abs", "../up"])]).unwrap_err();
    let all = why.join("\n");
    assert!(all.contains("`namespace` is missing"), "{all}");
    assert!(all.contains("`paths` is empty"), "{all}");
    assert!(all.contains("overlaps"), "{all}");
    assert!(all.contains("declared twice"), "{all}");
    assert!(all.contains("is absolute"), "{all}");
    assert!(all.contains("holds `.` or `..`"), "{all}");
}

#[test]
fn a_path_under_another_refs_path_overlaps_it_both_ways() {
    assert!(validate(&[decl("a", &["research"]), decl("b", &["research/notes"])]).is_err());
    assert!(validate(&[decl("a", &["research/notes"]), decl("b", &["research"])]).is_err());
    // the control: a sibling that only shares a name prefix does not overlap
    assert_eq!(validate(&[decl("a", &["research"]), decl("b", &["research_old"])]), Ok(()));
}

#[test]
fn a_form_that_is_neither_branch_nor_ref_is_refused() {
    let mut d = decl("a", &["x"]);
    d.form = Some("tag".into());
    let why = validate(&[d]).unwrap_err().join("\n");
    assert!(why.contains("neither `branch` nor `ref`"), "{why}");
}

#[test]
fn a_misspelt_key_refuses_the_table() {
    let parsed: Result<DataRefDecl, _> = toml_edit::de::from_str("namespace = \"a\"\npaths = [\"x\"]\npath = [\"y\"]\n");
    assert!(parsed.is_err());
    let ok: DataRefDecl = toml_edit::de::from_str("namespace = \"a\"\npaths = [\"x\"]\n").unwrap();
    assert_eq!(ok.paths, vec!["x".to_string()]);
}
