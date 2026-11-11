use std::path::Path;
use std::process::ExitCode;

use super::*;
use crate::config::Config;
use crate::registry::test_world::World;

fn git(dir: &Path, args: &[&str]) -> String {
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

fn write(cfg: &Config, rel: &str, text: &str) {
    let path = cfg.mock_dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A repository as a consumer has it before any migration: sources and templates side by side
/// and a research directory, all committed on the branch.
fn consumer(w: &World, name: &str) -> Config {
    let cfg = w.clone_of(name, false);
    write(&cfg, "crates/one/src/lib.rs", "pub fn one() {}\n");
    write(&cfg, "crates/one/DESIGN.md.tmpl", "design of one\n");
    write(&cfg, "crates/two/README.md.tmpl", "readme of two\n");
    write(&cfg, "research/note.md", "findings\n");
    write(&cfg, "research/deep/er.md", "more\n");
    git(&cfg.repo_root, &["add", "-f", "--", "."]);
    git(&cfg.repo_root, &["commit", "-q", "-m", "a consumer before the migration"]);
    cfg
}

fn tracked(cfg: &Config) -> Vec<String> {
    git(&cfg.repo_root, &["ls-files"]).lines().map(str::to_owned).collect()
}

const RESEARCH: [&str; 5] = ["directory-to-ref", "--namespace", "research", "--path", "research"];
const TEMPLATES: [&str; 7] =
    ["directory-to-ref", "--namespace", "templates", "--path", "crates", "--include", "**/*.md.tmpl"];

#[test]
fn a_call_that_names_no_migration_or_an_unknown_one_moves_nothing_and_lists_them() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    let before = tracked(&cfg);
    assert_eq!(run(&cfg, &[]), ExitCode::FAILURE);
    assert_eq!(run(&cfg, &["--dry-run"]), ExitCode::FAILURE);
    assert_eq!(run(&cfg, &["everything"]), ExitCode::FAILURE);
    assert_eq!(tracked(&cfg), before);
    assert!(names().contains(&"directory-to-ref") && names().contains(&"round-filenames"));
}

#[test]
fn a_dry_run_names_every_path_and_changes_nothing() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    let (before, config) = (tracked(&cfg), std::fs::read_to_string(&cfg.config_path).unwrap());
    let plan = plan(&cfg, &RESEARCH).unwrap();
    let listed = plan.lines.join("\n");
    assert!(listed.contains("mock/research/note.md") && listed.contains("mock/research/deep/er.md"), "{listed}");
    assert!(!listed.contains("lib.rs"), "{listed}");
    assert!(listed.contains("[[data_ref]]"), "the table it would write is shown: {listed}");

    let mut dry: Vec<&str> = RESEARCH.to_vec();
    dry.push("--dry-run");
    assert_eq!(run(&cfg, &dry), ExitCode::SUCCESS);
    assert_eq!(tracked(&cfg), before, "a dry run untracked a file");
    assert_eq!(std::fs::read_to_string(&cfg.config_path).unwrap(), config, "a dry run wrote the config");
    assert_eq!(git(&cfg.repo_root, &["for-each-ref", "refs/mock"]), "", "a dry run made a ref");
}

#[test]
fn a_directory_moves_to_its_ref_and_the_branch_stops_tracking_it() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);

    let after = tracked(&cfg);
    assert!(!after.iter().any(|p| p.starts_with("mock/research/")), "{after:?}");
    assert!(after.iter().any(|p| p == "mock/crates/one/src/lib.rs"), "the control: a source stays tracked");
    assert_eq!(std::fs::read_to_string(cfg.mock_dir.join("research/note.md")).unwrap(), "findings\n");

    // the config it wrote is one the engine reads, and the repository stands under it
    let again = Config::from_dir(&cfg.mock_dir);
    assert_eq!(again.data_refs.len(), 1);
    assert_eq!(crate::data_refs::standing(&again), Ok(()));
    assert_eq!(crate::data_refs::run(&again, &["verify"]), ExitCode::SUCCESS);

    // a second clone gets the files from the ref and none from the branch
    git(&cfg.repo_root, &["add", "--", "mock/mockspace.toml"]);
    git(&cfg.repo_root, &["commit", "-q", "-m", "move research to its ref"]);
    git(&cfg.repo_root, &["push", "-q", "origin", "HEAD"]);
    let b = w.clone_of("b", false);
    let branch = git(&cfg.repo_root, &["branch", "--show-current"]);
    git(&b.repo_root, &["fetch", "-q", "origin", branch.trim()]);
    git(&b.repo_root, &["reset", "-q", "--hard", "FETCH_HEAD"]);
    let b = Config::from_dir(&b.mock_dir);
    assert!(!b.mock_dir.join("research").exists(), "the control: the branch no longer carries it");
    assert_eq!(crate::data_refs::run(&b, &["instantiate"]), ExitCode::SUCCESS);
    assert_eq!(std::fs::read_to_string(b.mock_dir.join("research/deep/er.md")).unwrap(), "more\n");
}

#[test]
fn templates_move_by_pattern_and_the_sources_beside_them_stay_on_the_branch() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &TEMPLATES), ExitCode::SUCCESS);
    let after = tracked(&cfg);
    assert_eq!(after.iter().filter(|p| p.ends_with(".md.tmpl")).count(), 0, "{after:?}");
    assert!(after.iter().any(|p| p == "mock/crates/one/src/lib.rs"));
    assert!(cfg.mock_dir.join("crates/one/DESIGN.md.tmpl").is_file(), "the file stays on disk as the instance");
    let again = Config::from_dir(&cfg.mock_dir);
    assert_eq!(again.data_refs[0].include.as_deref(), Some(&["**/*.md.tmpl".to_string()][..]));
    assert_eq!(crate::data_refs::standing(&again), Ok(()));
}

#[test]
fn a_tree_the_migration_was_not_written_for_is_refused_with_the_reason() {
    let w = World::new();
    let cfg = consumer(&w, "a");

    // nothing tracked under the path
    let why = plan(&cfg, &["directory-to-ref", "--namespace", "benches", "--path", "benches"]).unwrap_err().join("\n");
    assert!(why.contains("tracks no file"), "{why}");

    // a held file changed and not committed: the ref would take what the branch never had
    write(&cfg, "research/note.md", "changed and not committed\n");
    let why = plan(&cfg, &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("mock/research/note.md") && why.contains("commit"), "{why}");
    git(&cfg.repo_root, &["checkout", "-q", "--", "mock/research/note.md"]);
    assert!(plan(&cfg, &RESEARCH).is_ok(), "the control: clean, the same call is planned");

    // done once, it is not done again
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    let again = Config::from_dir(&cfg.mock_dir);
    let why = plan(&again, &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("already"), "{why}");
}

#[test]
fn every_flag_is_named_and_nothing_is_guessed() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    for (args, word) in [
        (&["directory-to-ref", "--path", "research"][..], "--namespace"),
        (&["directory-to-ref", "--namespace", "research"][..], "--path"),
        (&["directory-to-ref", "--namespace", "research", "--path", "research", "--nope"][..], "--nope"),
        (&["directory-to-ref", "--namespace", "research", "--path"][..], "--path"),
        (&["directory-to-ref", "--namespace", "research", "--path", "research", "stray"][..], "stray"),
    ] {
        let why = plan(&cfg, args).unwrap_err().join("\n");
        assert!(why.contains(word), "{args:?}: {why}");
    }
    // a declaration the engine would refuse is refused before anything moves
    let why = plan(&cfg, &["directory-to-ref", "--namespace", "x", "--path", "registry"]).unwrap_err().join("\n");
    assert!(why.contains("store of its own"), "{why}");
}

#[test]
fn a_ref_that_already_has_commits_is_not_seeded_over() {
    let w = World::new();
    let a = consumer(&w, "a");
    assert_eq!(run(&a, &RESEARCH), ExitCode::SUCCESS);
    // a second clone that never took the config change still tracks the directory
    let b = consumer(&w, "b");
    let why = plan(&b, &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("already has commits"), "{why}");
    assert!(tracked(&b).iter().any(|p| p.starts_with("mock/research/")), "the refusal untracked files");
}
