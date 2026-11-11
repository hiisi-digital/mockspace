use std::process::ExitCode;

use super::*;
use crate::migrate::{plan as plan_of, run};

const NAME: &str = "references-to-letkajenkka";

const CONFIG: &str = r#"
[ref.roots.seed]
path = "corpus"
frozen = true

[[registry.namespace]]
key = "ruling"
title = "Ruling"

[[registry.namespace.field]]
name = "provenance"
type = "ref[]"

[[registry.namespace.field]]
name = "why"
type = "string"
"#;

const RULINGS: &str = r#"# a comment that stays
[[ruling]]
id = "first"
provenance = ["seed::DESIGN::#the-four-lanes", "seed::DESIGN::3", "nosuchroot::DESIGN::1"]
why = "see {{ seed::DESIGN::#warm-s-objective }} and {{ ruling::second }}"

[[ruling]]
id = "second"
provenance = ["seed::design::3"]
why = "escaped \t {{ seed::DESIGN::3 }}"

[[ruling]]
id = "third"
provenance = ["seed::design::3"]
why = "{{ seed::DESIGN::3\t}}"
"#;

const TEMPLATE: &str = "# Doc\n\nSee {{ seed::DESIGN::#the-four-lanes }}, {{ pathof( seed::Crates::My-Crate::NOTES::3 ) }}.\n\n```\n{{ seed::DESIGN::12 }}\n```\n\n{{ project_name }} {{ ruling::first }}\n";

struct Repo {
    _dir: tempfile::TempDir,
    cfg:  Config,
}

fn repo() -> Repo {
    repo_with("")
}

fn repo_with(extra_config: &str) -> Repo {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mockspace.toml"), format!("{CONFIG}{extra_config}")).unwrap();
    let cfg = Config::from_dir(dir.path());
    let put = |p: PathBuf, text: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    let corpus = cfg.repo_root.join("corpus");
    put(corpus.join("DESIGN.md"), "# Design\n\n## The four lanes\n\ntext\n\n## Warm's objective\n");
    put(corpus.join("Crates/My-Crate/NOTES.md"), "# Notes\n\nline\n\nmore\n");
    put(cfg.mock_dir.join("registry/rulings.toml"), RULINGS);
    put(cfg.repo_root.join("docs/DOC.md.tmpl"), TEMPLATE);
    Repo {
        _dir: dir,
        cfg,
    }
}

fn read(cfg: &Config, rel: &str) -> String {
    std::fs::read_to_string(cfg.repo_root.join(rel)).unwrap()
}

fn rel_of_rulings(cfg: &Config) -> String {
    cfg.mock_dir.join("registry/rulings.toml").strip_prefix(&cfg.repo_root).unwrap().to_string_lossy().into_owned()
}

// Mutation: a migration that rewrites only templates (never walks the registry directory) leaves
// the registry file at its old text and fails the second assertion; one that rewrites by text
// search instead of by the span of each string rewrites the comment and the escaped row too.
#[test]
fn a_run_rewrites_templates_and_registry_data_and_nothing_else_of_them() {
    let r = repo();
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    assert_eq!(
        read(&r.cfg, "docs/DOC.md.tmpl"),
        "# Doc\n\nSee {{ seed::design::the_four_lanes }}, {{ pathof(seed::crates::my_crate::notes::3) }}.\n\n```\n{{ seed::DESIGN::12 }}\n```\n\n{{ project_name }} {{ ruling::first }}\n"
    );
    let data = read(&r.cfg, &rel_of_rulings(&r.cfg));
    assert!(data.starts_with("# a comment that stays\n"), "{data}");
    assert!(
        data.contains(r#"provenance = ["seed::design::the_four_lanes", "seed::design::3", "nosuchroot::DESIGN::1"]"#),
        "{data}"
    );
    assert!(data.contains("{{ seed::design::warms_objective }} and {{ ruling::second }}"), "{data}");
    // an escape elsewhere in the string stays an escape, and one inside the reference is left
    assert!(data.contains(r#"why = "escaped \t {{ seed::design::3 }}""#), "{data}");
    assert!(data.contains(r#"why = "{{ seed::DESIGN::3\t}}""#), "{data}");
}

// Mutation: writing before the plan is shown, or ignoring `--dry-run`, changes a file here.
#[test]
fn a_dry_run_names_each_rewrite_and_each_reference_left_and_changes_nothing() {
    let r = repo();
    let before: Vec<String> = [&*rel_of_rulings(&r.cfg), "docs/DOC.md.tmpl"].iter().map(|p| read(&r.cfg, p)).collect();
    let plan = plan_of(&r.cfg, &[NAME]).unwrap();
    let said = plan.lines.join("\n");
    assert!(said.contains("rewrite docs/DOC.md.tmpl: seed::DESIGN::#the-four-lanes -> seed::design::the_four_lanes"), "{said}");
    assert!(said.contains("-> pathof(seed::crates::my_crate::notes::3)"), "{said}");
    assert!(said.contains("nosuchroot::DESIGN::1 (`nosuchroot` is no declared root"), "{said}");
    assert!(said.contains("left ") && said.contains("rewritten, 2 reference(s) left"), "{said}");
    assert_eq!(run(&r.cfg, &[NAME, "--dry-run"]), ExitCode::SUCCESS);
    let after: Vec<String> = [&*rel_of_rulings(&r.cfg), "docs/DOC.md.tmpl"].iter().map(|p| read(&r.cfg, p)).collect();
    assert_eq!(before, after);
}

// Mutation: a rewrite that is not a fixed point rewrites again on the second run, and a migration
// that errors when it finds nothing is not safe to run twice.
#[test]
fn a_second_run_changes_nothing_and_succeeds() {
    let r = repo();
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    let once: Vec<String> = [&*rel_of_rulings(&r.cfg), "docs/DOC.md.tmpl"].iter().map(|p| read(&r.cfg, p)).collect();
    let again = plan_of(&r.cfg, &[NAME]).unwrap();
    assert!(again.lines.iter().all(|l| !l.starts_with("rewrite ")), "{:?}", again.lines);
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    let twice: Vec<String> = [&*rel_of_rulings(&r.cfg), "docs/DOC.md.tmpl"].iter().map(|p| read(&r.cfg, p)).collect();
    assert_eq!(once, twice);
}

// Mutation: rewriting a name to something that reaches a different file (a fold that drops the
// `my_crate` step) leaves a citation that no longer resolves, which the gate below reports.
#[test]
fn what_was_rewritten_resolves_to_what_it_did_and_the_old_spelling_is_refused_beforehand() {
    let r = repo();
    let cfg = &r.cfg;
    let check = |cfg: &Config| {
        let reg = mockspace_registry::load_registry_for(cfg).unwrap();
        mockspace_registry::validate_provenance(
            &cfg.repo_root,
            &cfg.registry_roots,
            &cfg.frozen_roots,
            &reg,
            &cfg.registry_namespaces,
        )
    };
    let before = check(cfg);
    assert!(
        before.iter().any(|f| f.kind == "malformed-provenance" && f.message.contains(NAME)),
        "the old spelling is refused naming the migration: {before:?}"
    );
    let doc = |cfg: &Config| {
        let reg = mockspace_registry::load_registry_for(cfg).unwrap();
        mockspace_registry::resolve_all(
            &read(cfg, "docs/DOC.md.tmpl"),
            &cfg.registry_namespaces,
            &reg,
            &cfg.registry_roots,
            &cfg.repo_root,
            &cfg.repo_root.join("docs"),
            cfg,
        )
    };
    assert!(doc(cfg).contains("{{ seed::DESIGN::#the-four-lanes }}"), "the old spelling stays unresolved, as written");

    assert_eq!(run(cfg, &[NAME]), ExitCode::SUCCESS);
    let after = check(cfg);
    // the one left is the reference to a root nobody declared, and it is the only finding
    assert_eq!(after.iter().map(|f| f.kind).collect::<Vec<_>>(), ["malformed-provenance"], "{after:?}");
    let rendered = doc(cfg);
    assert!(rendered.contains("(../corpus/DESIGN.md#the-four-lanes)"), "{rendered}");
    assert!(rendered.contains("corpus/Crates/My-Crate/NOTES.md"), "{rendered}");
}

#[test]
fn a_call_with_a_flag_is_refused_and_moves_nothing() {
    let r = repo();
    let before = read(&r.cfg, "docs/DOC.md.tmpl");
    assert_eq!(run(&r.cfg, &[NAME, "--everything"]), ExitCode::FAILURE);
    assert_eq!(read(&r.cfg, "docs/DOC.md.tmpl"), before);
}

#[test]
fn a_registry_file_that_is_not_toml_stops_the_plan_and_is_named() {
    let r = repo();
    std::fs::write(r.cfg.mock_dir.join("registry/broken.toml"), "id = = =\n").unwrap();
    let why = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("broken.toml"), "{why}");
}

const LINTS: &str = "\n[lints.a]\nextends = \"stack-lints::no-heap\" # kept comment\n\n[lints.b]\nextends = 'Mockspace::No-Bare-Numeric'\n\n[lints.c]\nextends = \"stack-lints::9lives\"\n\n[lints.d]\nextends = \"mockspace::already_folded\"\n";

// Mutation: a migration that reads only templates and registry data leaves the config spelling alone.
#[test]
fn the_preset_shorthands_of_the_config_are_rewritten_and_the_one_that_cannot_be_is_left_and_named() {
    let r = repo_with(LINTS);
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    let config = read(&r.cfg, "mockspace.toml");
    assert!(config.contains("extends = \"stack_lints::no_heap\" # kept comment"), "{config}");
    assert!(config.contains("extends = 'mockspace::no_bare_numeric'"), "{config}");
    assert!(config.contains("extends = \"stack-lints::9lives\""), "{config}");
    assert!(config.contains("extends = \"mockspace::already_folded\""), "{config}");
    assert!(config.starts_with(CONFIG), "the rest of the file is as written");
    let said = plan_of(&r.cfg, &[NAME]).unwrap().lines.join("\n");
    assert!(said.contains("left mockspace.toml: stack-lints::9lives"), "{said}");
    assert!(!said.contains("rewrite mockspace.toml"), "a second run rewrites nothing: {said}");
}

// Mutation: ignoring `--dry-run` writes the config.
#[test]
fn a_dry_run_names_the_config_rewrites_and_leaves_the_config_as_written() {
    let r = repo_with(LINTS);
    let before = read(&r.cfg, "mockspace.toml");
    let said = plan_of(&r.cfg, &[NAME]).unwrap().lines.join("\n");
    assert!(said.contains("rewrite mockspace.toml: stack-lints::no-heap -> stack_lints::no_heap"), "{said}");
    assert!(said.contains("rewrite mockspace.toml: Mockspace::No-Bare-Numeric -> mockspace::no_bare_numeric"), "{said}");
    assert_eq!(run(&r.cfg, &[NAME, "--dry-run"]), ExitCode::SUCCESS);
    assert_eq!(read(&r.cfg, "mockspace.toml"), before);
}

#[test]
fn a_config_that_does_not_read_as_toml_after_the_rewrite_is_never_written() {
    let r = repo_with("\n[lints.a]\nextends = \"stack\\u002dlints::no-heap\"\n");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    let config = read(&r.cfg, "mockspace.toml");
    assert!(config.contains("extends = \"stack\\u002dlints::no-heap\""), "an escape inside the token is left: {config}");
}

const ROW_ON_A_REF: &str = "[[ruling]]\nid = \"first\"\ntitle = \"First\"\nprovenance = \"seed::DESIGN::#the-four-lanes, ruling::first\"\n";

// A registry kept on a ref is read and written through the registry's own stores, and its file is
// never written into the tree.
#[test]
fn a_registry_held_on_a_data_ref_is_rewritten_on_the_ref_and_not_in_the_tree() {
    let remote = mockspace_store::test_support::Remote::new();
    let root = remote.clone_dir("p");
    let config = format!("{CONFIG}\n[[registry.store]]\ndir = \"ruling\"\n");
    std::fs::write(root.join("mockspace.toml"), config).unwrap();
    std::fs::create_dir_all(root.join("corpus")).unwrap();
    std::fs::write(root.join("corpus/DESIGN.md"), "# Design\n\n## The four lanes\n\ntext\n").unwrap();
    let cfg = Config::from_dir(&root);
    mockspace_registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
    // The old spelling is refused by the writer, so the ref is made by hand, as a repository written
    // before the standard would have it.
    let seeder = remote.clone_dir("seeder");
    let git = |args: &[&str]| mockspace_store::test_support::git(&seeder, args);
    git(&["checkout", "-q", "--orphan", "seed"]);
    std::fs::write(seeder.join("first.toml"), ROW_ON_A_REF).unwrap();
    git(&["add", "first.toml"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "seed"]);
    git(&["push", "-q", "origin", "seed:refs/heads/mock/registry/ruling"]);
    assert!(!cfg.mock_dir.join("registry/ruling/first.toml").exists());

    let said = plan_of(&cfg, &[NAME]).unwrap().lines.join("\n");
    assert!(said.contains("(on its ref): seed::DESIGN::#the-four-lanes -> seed::design::the_four_lanes"), "{said}");
    assert_eq!(run(&cfg, &[NAME, "--dry-run"]), ExitCode::SUCCESS);
    let held = |remote: &mockspace_store::test_support::Remote| {
        remote.file("refs/heads/mock/registry/ruling", "first.toml").unwrap()
    };
    assert!(held(&remote).contains("seed::DESIGN::#the-four-lanes"), "a dry run writes no ref");

    assert_eq!(run(&cfg, &[NAME]), ExitCode::SUCCESS);
    let after = held(&remote);
    assert!(after.contains("seed::design::the_four_lanes"), "{after}");
    assert!(!cfg.mock_dir.join("registry/ruling/first.toml").exists(), "nothing is written into the tree");
    let again = plan_of(&cfg, &[NAME]).unwrap();
    assert!(again.lines.iter().all(|l| !l.starts_with("rewrite ")), "{:?}", again.lines);
}
