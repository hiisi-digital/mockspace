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

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

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
    git_in(dir.path(), &["init", "-q"]);
    git_in(dir.path(), &["add", "-A"]);
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

// A file to rewrite that is a symlink, or lies in a directory that is one, would be written through
// the link outside the repository, so the run refuses before it writes anything, the files of the
// tree as well, as a data ref refuses a write below a symlink.
//
// Mutation: writing each file with `fs::write` as it stands rewrites both files outside.
#[cfg(unix)]
#[test]
fn a_file_to_rewrite_at_or_below_a_symlink_is_refused_and_nothing_is_written() {
    let r = repo();
    let outside = tempfile::tempdir().unwrap();
    let (out_doc, out_rows) = (outside.path().join("OUT.md.tmpl"), outside.path().join("rows/more.toml"));
    let rows = "[[ruling]]\nid = \"fourth\"\nprovenance = [\"seed::DESIGN::3\"]\n";
    std::fs::write(&out_doc, TEMPLATE).unwrap();
    std::fs::create_dir_all(out_rows.parent().unwrap()).unwrap();
    std::fs::write(&out_rows, rows).unwrap();
    std::os::unix::fs::symlink(&out_doc, r.cfg.repo_root.join("docs/LINKED.md.tmpl")).unwrap();
    std::os::unix::fs::symlink(out_rows.parent().unwrap(), r.cfg.mock_dir.join("registry/linked")).unwrap();
    git_in(&r.cfg.repo_root, &["add", "-A"]);
    let before = read(&r.cfg, "docs/DOC.md.tmpl");
    let why = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("docs/LINKED.md.tmpl is a symlink"), "{why}");
    assert!(why.contains("registry/linked is a symlink"), "{why}");
    assert_ne!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    assert_eq!(std::fs::read_to_string(&out_doc).unwrap(), TEMPLATE);
    assert_eq!(std::fs::read_to_string(&out_rows).unwrap(), rows);
    assert_eq!(read(&r.cfg, "docs/DOC.md.tmpl"), before);
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

// A migration already made is refused with the reason, and moves nothing, per the design of named
// migrations. A run stopped part way is finished by the same call.
//
// Mutation: a migration that reports success on a tree with nothing to rewrite is not refusing one
// already made, and one that refuses whenever any reference was rewritten earlier cannot finish a
// stopped run.
#[test]
fn a_tree_already_migrated_is_refused_and_a_half_migrated_one_is_finished() {
    let r = repo();
    // stopped part way: the template was rewritten, the registry data was not
    let half = read(&r.cfg, "docs/DOC.md.tmpl").replace("seed::DESIGN::#the-four-lanes", "seed::design::the_four_lanes");
    std::fs::write(r.cfg.repo_root.join("docs/DOC.md.tmpl"), half).unwrap();
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS, "the same call finishes it");
    let once: Vec<String> = [&*rel_of_rulings(&r.cfg), "docs/DOC.md.tmpl"].iter().map(|p| read(&r.cfg, p)).collect();
    let why = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("already"), "{why}");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::FAILURE);
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
    let said = plan_of(&r.cfg, &[NAME]).unwrap_err().join("\n");
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
    let again = plan_of(&cfg, &[NAME]).unwrap_err().join("\n");
    assert!(again.contains("already"), "{again}");
}

fn put_in(cfg: &Config, rel: &str, text: &str) {
    let p = cfg.repo_root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

// The templates are what git lists of the repository, tracked or not yet added but not ignored, and
// never what a walk of the directory tree finds: an ignored directory is not the repository's.
//
// Mutation: a walk of the tree rewrites the template in the ignored directory, and one that asks
// git for the tracked files only misses the one not yet added.
#[test]
fn the_templates_are_the_ones_git_lists_and_an_ignored_directory_is_not_walked_into() {
    let r = repo();
    put_in(&r.cfg, ".gitignore", "scratch/\n");
    put_in(&r.cfg, "scratch/X.md.tmpl", TEMPLATE);
    put_in(&r.cfg, "docs/NEW.md.tmpl", TEMPLATE);
    let said = plan_of(&r.cfg, &[NAME]).unwrap().lines.join("\n");
    assert!(said.contains("rewrite docs/NEW.md.tmpl:"), "{said}");
    assert!(!said.contains("scratch"), "{said}");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    assert_eq!(read(&r.cfg, "scratch/X.md.tmpl"), TEMPLATE, "an ignored file is left as it was");
    assert!(read(&r.cfg, "docs/NEW.md.tmpl").contains("seed::design::the_four_lanes"));
}

// Without git there is no list of the repository's files, so the run says so and moves nothing.
#[test]
fn a_directory_that_is_no_repository_is_refused_naming_git() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mockspace.toml"), CONFIG).unwrap();
    std::fs::write(dir.path().join("A.md.tmpl"), TEMPLATE).unwrap();
    let cfg = Config::from_dir(dir.path());
    let why = plan_of(&cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("git"), "{why}");
    assert_eq!(std::fs::read_to_string(dir.path().join("A.md.tmpl")).unwrap(), TEMPLATE);
}

const ARRAYS: &str = "[lints.a]\nextends = \"stack-lints::no-heap\"\n\n[lints.b]\nextends = [\"Mockspace::No-Bare-Numeric\", \"stack-lints::9lives\"]\n";

// The v2 loader reads `lints.toml` and `mock/lints.toml` before `mockspace.toml`, so a spelling there
// is read as surely as one in the config, and an array of presets is read item by item.
//
// Mutation: a migration of `mockspace.toml` alone leaves both files as they were, and one that
// reads only string values leaves the array.
#[test]
fn every_file_the_loaders_read_for_presets_is_rewritten_and_an_array_of_them_item_by_item() {
    let r = repo();
    put_in(&r.cfg, "lints.toml", ARRAYS);
    put_in(&r.cfg, "mock/lints.toml", ARRAYS);
    git_in(&r.cfg.repo_root, &["add", "-A"]);
    let said = plan_of(&r.cfg, &[NAME]).unwrap().lines.join("\n");
    for file in ["lints.toml", "mock/lints.toml"] {
        assert!(said.contains(&format!("rewrite {file}: stack-lints::no-heap -> stack_lints::no_heap")), "{said}");
        assert!(said.contains(&format!("rewrite {file}: Mockspace::No-Bare-Numeric -> mockspace::no_bare_numeric")), "{said}");
        assert!(said.contains(&format!("left {file}: stack-lints::9lives")), "{said}");
    }
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    for file in ["lints.toml", "mock/lints.toml"] {
        let text = read(&r.cfg, file);
        assert_eq!(
            text,
            "[lints.a]\nextends = \"stack_lints::no_heap\"\n\n[lints.b]\nextends = [\"mockspace::no_bare_numeric\", \"stack-lints::9lives\"]\n",
            "{file}"
        );
    }
}

use crate::registry::engine_world::EngineClone as _;
use crate::registry::test_world::World;

// A data ref declared and not instantiated in this clone holds files the run cannot read, and a run
// that skipped it would call the tree migrated with its templates and data untouched.
//
// Mutation: skipping a ref with no instance reports the rest as the whole.
#[test]
fn a_declared_data_ref_with_no_instance_in_the_clone_is_refused_naming_the_way_out() {
    let w = World::new();
    let cfg = w.clone_of("a", false);
    let mut text = std::fs::read_to_string(&cfg.config_path).unwrap_or_default();
    text.push_str(&format!("\n{CONFIG}\n[[data_ref]]\nnamespace = \"templates\"\npath = \"crates\"\ninclude = [\"**/*.md.tmpl\"]\n"));
    std::fs::write(&cfg.config_path, text).unwrap();
    let cfg = Config::from_dir(&cfg.mock_dir);
    assert_eq!(cfg.data_refs.len(), 1);
    let why = plan_of(&cfg, &[NAME]).unwrap_err().join("\n");
    assert!(why.contains("templates") && why.contains("data-refs instantiate"), "{why}");
}

// Files held on a ref are written into its instance, which is no commit: the next step is the one that
// makes it, and a run that touched no instance does not name it.
#[test]
fn a_rewrite_under_a_data_ref_instance_names_data_refs_commit_as_the_next_step() {
    let w = World::new();
    let cfg = w.clone_of("a", false);
    put_in(&cfg, "mock/crates/one/DESIGN.md.tmpl", TEMPLATE);
    put_in(&cfg, "mock/crates/one/src/lib.rs", "pub fn one() {}\n");
    put_in(&cfg, "corpus/DESIGN.md", "# Design\n\n## The four lanes\n\ntext\n");
    put_in(&cfg, "corpus/Crates/My-Crate/NOTES.md", "# Notes\n\nline\n\nmore\n");
    let mut text = std::fs::read_to_string(&cfg.config_path).unwrap_or_default();
    text.push_str(&format!("\n{CONFIG}\n"));
    std::fs::write(&cfg.config_path, text).unwrap();
    git_in(&cfg.repo_root, &["add", "-f", "--", "."]);
    git_in(&cfg.repo_root, &["commit", "-q", "-m", "a consumer"]);
    let cfg = Config::from_dir(&cfg.mock_dir);
    assert_eq!(
        crate::migrate::run(&cfg, &["directory-to-ref", "--namespace", "templates", "--path", "crates", "--include", "**/*.md.tmpl"]),
        ExitCode::SUCCESS
    );
    let cfg = Config::from_dir(&cfg.mock_dir);
    let plan = plan_of(&cfg, &[NAME]).unwrap();
    assert!(plan.after().contains("data-refs commit"), "{}", plan.after());
    assert_eq!(run(&cfg, &[NAME]), ExitCode::SUCCESS);
    assert!(std::fs::read_to_string(cfg.mock_dir.join("crates/one/DESIGN.md.tmpl")).unwrap().contains("seed::design::the_four_lanes"));

    // the control: a tree whose rewrites touch no instance says only to commit them
    let r = repo();
    assert!(!plan_of(&r.cfg, &[NAME]).unwrap().after().contains("data-refs"));
}

// What a dry run names is what the real run writes: the same paths and the same edits.
//
// Mutation: a plan whose lines and writes are built apart can name a file it does not write, or
// write one it does not name; the set of files that changed is compared with the set named.
#[test]
fn the_dry_run_names_exactly_the_paths_and_edits_the_real_run_writes() {
    let r = repo_with(LINTS);
    put_in(&r.cfg, "lints.toml", ARRAYS);
    put_in(&r.cfg, "docs/NEW.md.tmpl", TEMPLATE);
    git_in(&r.cfg.repo_root, &["add", "-A"]);
    let snapshot = |cfg: &Config| -> std::collections::BTreeMap<String, String> {
        git_in(&cfg.repo_root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| (p.to_string(), std::fs::read_to_string(cfg.repo_root.join(p)).unwrap()))
            .collect()
    };
    let before = snapshot(&r.cfg);
    let named: Vec<String> = plan_of(&r.cfg, &[NAME]).unwrap().lines.iter().filter(|l| l.starts_with("rewrite ")).cloned().collect();
    assert_eq!(run(&r.cfg, &[NAME, "--dry-run"]), ExitCode::SUCCESS);
    assert_eq!(snapshot(&r.cfg), before, "a dry run writes nothing");
    assert_eq!(run(&r.cfg, &[NAME]), ExitCode::SUCCESS);
    let after = snapshot(&r.cfg);
    let changed: std::collections::BTreeSet<&String> = before.keys().filter(|k| before[*k] != after[*k]).collect();
    let said: std::collections::BTreeSet<String> = named
        .iter()
        .map(|l| l["rewrite ".len() ..].split(": ").next().unwrap().to_string())
        .collect();
    assert_eq!(changed.iter().map(|s| s.to_string()).collect::<std::collections::BTreeSet<_>>(), said);
    for line in &named {
        let (path, edit) = line["rewrite ".len() ..].split_once(": ").unwrap();
        let new = edit.split(" -> ").nth(1).unwrap();
        assert!(after[path].contains(new), "{line}");
    }
}

fn in_rulings(text: &str) -> (String, Vec<Found>) {
    let dir = tempfile::tempdir().unwrap();
    let (roots, ns) = (BTreeMap::new(), BTreeSet::new());
    let ctx = Ctx {
        repo_root:  dir.path(),
        roots:      &roots,
        namespaces: &ns,
    };
    let fields = BTreeMap::from([("ruling".to_string(), vec!["provenance".to_string()])]);
    in_toml(text, &fields, &ctx).unwrap()
}

// Two items of one list where the first is the start of the second, and the same item again after
// them: each is rewritten where it stands.
//
// Mutation: rewriting the string by a search for each item's text, as the run did, writes
// `crates::foo-bar`, which is no name, and leaves the second item's own rewrite nothing to find.
#[test]
fn a_list_whose_item_starts_another_is_rewritten_item_by_item() {
    let (new, found) = in_rulings("[[ruling]]\nid = \"a\"\nprovenance = [\"crates::Foo, crates::Foo-bar, crates::Foo\"]\n");
    assert_eq!(new, "[[ruling]]\nid = \"a\"\nprovenance = [\"crates::foo, crates::foo_bar, crates::foo\"]\n");
    assert_eq!(found.len(), 3, "{found:?}");
}

// Mutation: taking the bytes of the value as the bytes of the file rewrites at the wrong place
// once an escape stands before the reference, and one inside the reference is not noticed.
#[test]
fn a_reference_with_an_escape_inside_is_left_and_one_after_an_escape_is_rewritten_in_place() {
    let (new, found) = in_rulings("[[ruling]]\nprovenance = [\"\\u0063rates::Foo-bar, crates::Foo\"]\n");
    assert_eq!(new, "[[ruling]]\nprovenance = [\"\\u0063rates::Foo-bar, crates::foo\"]\n");
    let left: Vec<&str> = found.iter().filter(|f| matches!(f.outcome, Outcome::Left(_))).map(|f| f.token.as_str()).collect();
    assert_eq!(left, ["crates::Foo-bar"]);
}

// Each form of a TOML string, and which byte of the file writes each byte of its value.
//
// Mutation: not dropping the line break after the opening quotes of a multi-line string, or the
// whitespace after a line-ending backslash, shifts every byte after it, and the check against the
// value refuses the string.
#[test]
fn each_byte_of_a_value_is_traced_to_the_byte_of_the_file_that_writes_it() {
    assert_eq!(written_at("\"a\\tb\"", "a\tb"), Some(vec![Some(1), None, Some(4)]));
    assert_eq!(written_at("'a\\tb'", "a\\tb"), Some(vec![Some(1), Some(2), Some(3), Some(4)]));
    assert_eq!(written_at("\"\"\"\nab\"\"\"", "ab"), Some(vec![Some(4), Some(5)]));
    assert_eq!(written_at("\"\"\"a\\\n  b\"\"\"", "ab"), Some(vec![Some(3), Some(8)]));
    assert_eq!(written_at("\"\\u00e4x\"", "äx"), Some(vec![None, None, Some(7)]));
    assert_eq!(written_at("\"ab\"", "ax"), None, "a value the bytes do not spell is not read");
    assert_eq!(raw_span(&[Some(1), None, Some(4)], &(0 .. 1)), Some(1 .. 2));
    assert_eq!(raw_span(&[Some(1), None, Some(4)], &(0 .. 3)), None);
}

#[cfg(unix)]
#[path = "migrate_refs_bytes_tests.rs"]
mod bytes;
