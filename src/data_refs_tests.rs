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
    assert_eq!(validate(&[decl("notes", "notes"), decl("research", "research")]), Ok(()));
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

#[test]
fn a_path_under_a_store_of_its_own_is_refused() {
    for p in ["registry", "registry/ticket", "design_rounds", "test-history"] {
        let why = validate(&[decl("a", p)]).unwrap_err().join("\n");
        assert!(why.contains("has a store of its own"), "{p}: {why}");
    }
    // the control: a directory that only starts like one is fine
    assert_eq!(validate(&[decl("a", "registry_old")]), Ok(()));
}

fn held(ns: &str, path: &str, include: &[&str]) -> DataRefDecl {
    DataRefDecl {
        include: Some(include.iter().map(|p| (*p).to_string()).collect()),
        ..decl(ns, path)
    }
}

#[test]
fn a_ref_may_hold_the_files_of_a_directory_that_patterns_name() {
    assert_eq!(validate(&[held("templates", "crates", &["**/*.md.tmpl"])]), Ok(()));
    let why = validate(&[held("templates", "crates", &["what?"])]).unwrap_err().join("\n");
    assert!(why.contains("`include`") && why.contains("what?"), "{why}");
    let why = validate(&[held("templates", "crates", &[])]).unwrap_err().join("\n");
    assert!(why.contains("no pattern is given"), "{why}");
    // files held by pattern lie among the branch's, so they cannot be placed elsewhere
    let mut moved = held("templates", "crates", &["**/*.md.tmpl"]);
    moved.instance = Some("elsewhere".into());
    let why = validate(&[moved]).unwrap_err().join("\n");
    assert!(why.contains("`instance` and `include` are both given"), "{why}");
}

#[test]
fn the_closed_rounds_may_go_to_a_ref_and_a_flat_round_file_may_not() {
    assert_eq!(validate(&[held("rounds", "design_rounds", &["*/**"])]), Ok(()));
    for flat in ["*.md", "**", "**/*.md", "topic.md"] {
        let why = validate(&[held("rounds", "design_rounds", &[flat])]).unwrap_err().join("\n");
        assert!(why.contains("open") && why.contains(flat), "{flat}: {why}");
    }
    // the control: without patterns the directory is refused whole, as it was
    let why = validate(&[decl("rounds", "design_rounds")]).unwrap_err().join("\n");
    assert!(why.contains("has a store of its own"), "{why}");
    // and patterns do not open the registry or a directory inside the rounds
    for p in ["registry", "design_rounds/202611010000"] {
        let why = validate(&[held("a", p, &["*/**"])]).unwrap_err().join("\n");
        assert!(why.contains("has a store of its own"), "{p}: {why}");
    }
}

#[test]
fn two_refs_may_not_place_their_instances_over_each_other() {
    let mut b = decl("b", "other");
    b.instance = Some("research/notes".into());
    let why = validate(&[decl("a", "research"), b.clone()]).unwrap_err().join("\n");
    assert!(why.contains("instance") && why.contains("overlaps"), "{why}");
    // the control: an instance elsewhere is fine
    b.instance = Some("elsewhere".into());
    assert_eq!(validate(&[decl("a", "research"), b.clone()]), Ok(()));
}

#[test]
fn an_instance_inside_a_store_of_its_own_is_refused() {
    for p in ["registry", "design_rounds", "test-history/x"] {
        let mut d = decl("a", "notes");
        d.instance = Some(p.into());
        let why = validate(&[d]).unwrap_err().join("\n");
        assert!(why.contains("instance") && why.contains("has a store of its own"), "{p}: {why}");
    }
    // the control: a sibling that only shares a prefix is fine
    let mut d = decl("a", "notes");
    d.instance = Some("registry_old".into());
    assert_eq!(validate(&[d]), Ok(()));
}

#[test]
fn a_namespace_a_registry_store_or_the_history_writes_is_refused() {
    let taken = vec!["registry/ticket".to_string(), "test-history".to_string()];
    for ns in ["registry/ticket", "test-history"] {
        let why = validate_against(&[decl(ns, "x")], &taken).unwrap_err().join("\n");
        assert!(why.contains("registry store"), "{ns}: {why}");
    }
    // the control: a namespace of its own passes against the same list
    assert_eq!(validate_against(&[decl("research", "x")], &taken), Ok(()));
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
    fn templates_held_by_pattern_travel_and_the_sources_beside_them_do_not() {
        let w = World::new();
        let mut a = w.clone_of("a", false);
        a.data_refs = vec![held("templates", "crates", &["**/*.md.tmpl"])];
        std::fs::create_dir_all(a.mock_dir.join("crates/one/src")).unwrap();
        std::fs::write(a.mock_dir.join("crates/one/src/lib.rs"), "pub fn one() {}\n").unwrap();
        std::fs::write(a.mock_dir.join("crates/one/DESIGN.md.tmpl"), "design of one\n").unwrap();
        assert_eq!(run(&a, &["commit"]), ExitCode::SUCCESS);
        assert_eq!(run(&a, &["verify"]), ExitCode::SUCCESS, "a source beside the template is no change");

        let mut b = w.clone_of("b", false);
        b.data_refs = a.data_refs.clone();
        assert_eq!(run(&b, &["instantiate"]), ExitCode::SUCCESS);
        assert_eq!(std::fs::read_to_string(b.mock_dir.join("crates/one/DESIGN.md.tmpl")).unwrap(), "design of one\n");
        assert!(!b.mock_dir.join("crates/one/src/lib.rs").exists(), "the control: a source came out of a data ref");
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
    fn status_without_an_instance_fails_and_names_instantiate() {
        let w = World::new();
        let a = with_ref(&w, "a");
        assert_eq!(run(&a, &["status"]), ExitCode::FAILURE, "nothing is instantiated yet");
        std::fs::create_dir_all(a.mock_dir.join("research")).unwrap();
        std::fs::write(a.mock_dir.join("research/note.md"), "findings\n").unwrap();
        assert_eq!(run(&a, &["commit"]), ExitCode::SUCCESS);
        assert_eq!(run(&a, &["status"]), ExitCode::SUCCESS, "the control: with the instance present it passes");
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
