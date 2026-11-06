use super::*;

fn decl(ns: &str, path: &str) -> DataRefDecl {
    DataRefDecl {
        namespace: Some(ns.into()),
        path: Some(path.to_string()),
        ..DataRefDecl::default()
    }
}

#[test]
fn declarations_with_a_namespace_and_a_path_each_are_accepted() {
    assert_eq!(validate(&[decl("registry", "registry"), decl("research", "research")]), Ok(()));
}

#[test]
fn nothing_declared_is_accepted() {
    assert_eq!(validate(&[]), Ok(()));
}

#[test]
fn every_mistake_is_named_in_one_run() {
    let why = validate(&[DataRefDecl::default(), decl("a", "x"), decl("a", "x/y"), decl("b", "/abs"), decl("c", "../up")])
        .unwrap_err();
    let all = why.join("\n");
    assert!(all.contains("`namespace` is missing"), "{all}");
    assert!(all.contains("`path` is missing"), "{all}");
    assert!(all.contains("overlaps"), "{all}");
    assert!(all.contains("declared twice"), "{all}");
    assert!(all.contains("is absolute"), "{all}");
    assert!(all.contains("holds `.` or `..`"), "{all}");
}

#[test]
fn a_path_under_another_refs_path_overlaps_it_both_ways() {
    assert!(validate(&[decl("a", "research"), decl("b", "research/notes")]).is_err());
    assert!(validate(&[decl("a", "research/notes"), decl("b", "research")]).is_err());
    // the control: a sibling that only shares a name prefix does not overlap
    assert_eq!(validate(&[decl("a", "research"), decl("b", "research_old")]), Ok(()));
}

#[test]
fn a_form_that_is_neither_branch_nor_ref_is_refused() {
    let mut d = decl("a", "x");
    d.form = Some("tag".into());
    let why = validate(&[d]).unwrap_err().join("\n");
    assert!(why.contains("neither `branch` nor `ref`"), "{why}");
}

#[test]
fn a_misspelt_key_refuses_the_table() {
    let parsed: Result<DataRefDecl, _> = toml_edit::de::from_str("namespace = \"a\"\npath = \"x\"\npaths = [\"y\"]\n");
    assert!(parsed.is_err());
    let ok: DataRefDecl = toml_edit::de::from_str("namespace = \"a\"\npath = \"x\"\n").unwrap();
    assert_eq!(ok.path.as_deref(), Some("x"));
}

mod verbs {
    use std::process::ExitCode;

    use super::*;
    use crate::registry::test_world::World;

    fn with_ref(w: &World, name: &str) -> crate::config::Config {
        let mut cfg = w.clone_of(name, false);
        cfg.data_refs = vec![decl("research", "research")];
        cfg
    }

    #[test]
    fn a_directory_committed_in_one_clone_is_instantiated_in_another() {
        let w = World::new();
        let a = with_ref(&w, "a");
        std::fs::create_dir_all(a.mock_dir.join("research")).unwrap();
        std::fs::write(a.mock_dir.join("research/note.md"), "findings\n").unwrap();
        assert_eq!(run(&a, &["commit"]), ExitCode::SUCCESS);

        let b = with_ref(&w, "b");
        assert!(!b.mock_dir.join("research").exists(), "the control: b holds nothing before it instantiates");
        assert_eq!(run(&b, &["instantiate"]), ExitCode::SUCCESS);
        assert_eq!(std::fs::read_to_string(b.mock_dir.join("research/note.md")).unwrap(), "findings\n");
    }

    #[test]
    fn verify_passes_on_a_clean_instance_and_names_what_is_wrong_otherwise() {
        let w = World::new();
        let a = with_ref(&w, "a");
        assert_eq!(run(&a, &["verify"]), ExitCode::FAILURE, "the control: nothing is instantiated yet");
        std::fs::create_dir_all(a.mock_dir.join("research")).unwrap();
        std::fs::write(a.mock_dir.join("research/note.md"), "findings\n").unwrap();
        assert_eq!(run(&a, &["commit"]), ExitCode::SUCCESS);
        assert_eq!(run(&a, &["verify"]), ExitCode::SUCCESS);
        std::fs::write(a.mock_dir.join("research/note.md"), "changed\n").unwrap();
        assert_eq!(run(&a, &["verify"]), ExitCode::FAILURE, "the control: a changed file is not verified");
    }

    #[test]
    fn a_namespace_nothing_declares_is_a_failure_and_none_declared_is_not() {
        let w = World::new();
        let cfg = with_ref(&w, "a");
        assert_eq!(run(&cfg, &["status", "nope"]), ExitCode::FAILURE);
        let mut none = w.clone_of("b", false);
        none.data_refs = Vec::new();
        assert_eq!(run(&none, &["status"]), ExitCode::SUCCESS);
        assert_eq!(run(&cfg, &["bogus"]), ExitCode::FAILURE);
    }
}
