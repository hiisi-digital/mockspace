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
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mockspace.toml"), CONFIG).unwrap();
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
