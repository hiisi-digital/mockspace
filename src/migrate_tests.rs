use std::path::Path;
use std::process::ExitCode;

use super::*;
use crate::config::Config;
use crate::registry::engine_world::EngineClone as _;
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
    // a second clone that never took the config change still tracks the directory. Its files are
    // the ref's, which is what a run stopped inside its seed leaves, so the call finishes there
    let b = consumer(&w, "b");
    let finishing = plan(&b, &RESEARCH).unwrap().lines.join("\n");
    assert!(finishing.contains("did not record") && !finishing.contains("make the ref"), "{finishing}");
    // with other files than the ref's it is another tree, and nothing is seeded over the ref
    write(&b, "research/note.md", "another clone's findings\n");
    git(&b.repo_root, &["commit", "-q", "-am", "other findings"]);
    let why = plan(&b, &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("already has commits"), "{why}");
    assert_eq!(run(&b, &RESEARCH), ExitCode::FAILURE);
    assert!(tracked(&b).iter().any(|p| p.starts_with("mock/research/")), "the refusal untracked files");
}

#[test]
fn the_rest_of_the_mock_directory_moves_to_one_ref_and_the_sources_stay() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    write(&cfg, "catalogue.md", "the catalogue\n");
    write(&cfg, "design_rounds/202611010000/topic.md", "closed\n");
    write(&cfg, "design_rounds/202611110000_topic.open.md", "open\n");
    git(&cfg.repo_root, &["add", "-f", "--", "."]);
    git(&cfg.repo_root, &["commit", "-q", "-m", "more of a mock directory"]);
    let call = [
        "directory-to-ref", "--namespace", "mock", "--path", ".",
        "--include", "research/**", "--include", "design_rounds/*/**",
        "--include", "crates/**/*.md.tmpl", "--include", "catalogue.md",
    ];
    assert_eq!(run(&cfg, &call), ExitCode::SUCCESS);
    let mut after = tracked(&cfg);
    after.retain(|p| p.starts_with("mock/") && p != "mock/mockspace.toml");
    assert_eq!(after, ["mock/crates/one/src/lib.rs", "mock/design_rounds/202611110000_topic.open.md"]);
    let again = Config::from_dir(&cfg.mock_dir);
    assert_eq!(crate::data_refs::standing(&again), Ok(()));
    assert_eq!(crate::data_refs::run(&again, &["verify"]), ExitCode::SUCCESS);
    assert!(cfg.mock_dir.join("catalogue.md").is_file() && cfg.mock_dir.join("research/deep/er.md").is_file());
}

#[test]
fn the_gates_are_told_which_held_files_changed() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &TEMPLATES), ExitCode::SUCCESS);
    let cfg = Config::from_dir(&cfg.mock_dir);
    let files = crate::data_refs::ref_files(&cfg).unwrap();
    let held: Vec<&str> = files.held.iter().map(String::as_str).collect();
    assert_eq!(held, ["crates/one/DESIGN.md.tmpl", "crates/two/README.md.tmpl"]);
    assert!(files.changed.is_empty(), "{:?}", files.changed);

    write(&cfg, "crates/one/DESIGN.md.tmpl", "a new design of one\n");
    write(&cfg, "crates/one/NOTES.md.tmpl", "added\n");
    std::fs::remove_file(cfg.mock_dir.join("crates/two/README.md.tmpl")).unwrap();
    write(&cfg, "crates/one/src/lib.rs", "pub fn one() -> u8 { 1 }\n");
    let files = crate::data_refs::ref_files(&cfg).unwrap();
    let changed: Vec<&str> = files.changed.iter().map(String::as_str).collect();
    assert_eq!(changed, ["crates/one/DESIGN.md.tmpl", "crates/one/NOTES.md.tmpl", "crates/two/README.md.tmpl"]);
    assert!(files.held.contains("crates/one/NOTES.md.tmpl") && files.held.contains("crates/two/README.md.tmpl"));
    assert!(!files.held.contains("crates/one/src/lib.rs"), "a source is the branch's, and git judges it");
}

#[test]
fn round_files_are_renamed_and_two_that_would_take_one_name_are_refused() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    // nothing legacy: not a tree it was written for
    std::fs::create_dir_all(cfg.mock_dir.join("design_rounds")).unwrap();
    let why = plan(&cfg, &["round-filenames"]).unwrap_err().join("\n");
    assert!(why.contains("not a tree the migration was written for"), "{why}");

    write(&cfg, "design_rounds/2026-03-07_changelist.md", "first changelist\n");
    let planned = plan(&cfg, &["round-filenames"]).unwrap().lines.join("\n");
    assert!(planned.contains("mock/design_rounds/2026-03-07_changelist.md"), "{planned}");

    // a second file whose new name is the first one's: renaming both would lose one
    write(&cfg, "design_rounds/2026-03-07_foo_changelist.md", "second changelist\n");
    let why = plan(&cfg, &["round-filenames"]).unwrap_err().join("\n");
    assert!(why.contains("would both be renamed"), "{why}");
    assert_eq!(run(&cfg, &["round-filenames"]), ExitCode::FAILURE);
    let dir = cfg.mock_dir.join("design_rounds");
    assert_eq!(std::fs::read_to_string(dir.join("2026-03-07_changelist.md")).unwrap(), "first changelist\n");
    assert_eq!(std::fs::read_to_string(dir.join("2026-03-07_foo_changelist.md")).unwrap(), "second changelist\n");

    // the control: alone again, the first is renamed and keeps its text
    std::fs::remove_file(dir.join("2026-03-07_foo_changelist.md")).unwrap();
    assert_eq!(run(&cfg, &["round-filenames"]), ExitCode::SUCCESS);
    assert!(!dir.join("2026-03-07_changelist.md").exists());
    let renamed: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().is_file()).collect();
    assert_eq!(renamed.len(), 1);
    assert_eq!(std::fs::read_to_string(renamed[0].path()).unwrap(), "first changelist\n");
}

#[test]
fn a_run_stopped_after_the_config_was_written_is_finished_by_the_same_call() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    // a held index stops the last step: the ref is pushed, the table written, the files tracked
    let lock = cfg.repo_root.join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::FAILURE);
    std::fs::remove_file(&lock).unwrap();
    let again = Config::from_dir(&cfg.mock_dir);
    assert_eq!(again.data_refs.len(), 1, "the control: the run got as far as the config");
    assert!(tracked(&again).iter().any(|p| p.starts_with("mock/research/")), "the control: and no further");

    let finishing = plan(&again, &RESEARCH).unwrap().lines.join("\n");
    assert!(finishing.contains("finishing") && !finishing.contains("[[data_ref]]"), "{finishing}");
    assert_eq!(run(&again, &RESEARCH), ExitCode::SUCCESS);
    assert!(!tracked(&again).iter().any(|p| p.starts_with("mock/research/")));
    let done = Config::from_dir(&cfg.mock_dir);
    assert_eq!(done.data_refs.len(), 1, "the table was written twice");
    assert_eq!(crate::data_refs::standing(&done), Ok(()));
    // and finished, it is not done a third time
    assert!(plan(&done, &RESEARCH).unwrap_err().join("\n").contains("already"));
}

#[test]
fn a_run_stopped_after_the_ref_was_made_is_finished_by_the_same_call() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    let before = std::fs::read_to_string(&cfg.config_path).unwrap();
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    // as it stands where the config step failed: the ref made here, no table, the files tracked
    std::fs::write(&cfg.config_path, &before).unwrap();
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    let again = Config::from_dir(&cfg.mock_dir);
    assert!(again.data_refs.is_empty());

    let finishing = plan(&again, &RESEARCH).unwrap().lines.join("\n");
    assert!(finishing.contains("finishing") && finishing.contains("[[data_ref]]"), "{finishing}");
    assert_eq!(run(&again, &RESEARCH), ExitCode::SUCCESS);
    let done = Config::from_dir(&cfg.mock_dir);
    assert_eq!(done.data_refs.len(), 1);
    assert_eq!(crate::data_refs::standing(&done), Ok(()));
    assert_eq!(crate::data_refs::run(&done, &["verify"]), ExitCode::SUCCESS);

    // the control: a directory changed since the ref was made is not finished over
    std::fs::write(&cfg.config_path, &before).unwrap();
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    write(&cfg, "research/note.md", "changed since\n");
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    git(&cfg.repo_root, &["commit", "-q", "-m", "changed since"]);
    let why = plan(&Config::from_dir(&cfg.mock_dir), &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("changed since"), "{why}");
}

#[test]
fn a_run_stopped_inside_the_seed_is_finished_by_the_same_call() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    let before = std::fs::read_to_string(&cfg.config_path).unwrap();
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    // as it stands where the seed pushed and was stopped before it recorded the directory: the
    // ref made, no record, no table, the files tracked
    std::fs::write(&cfg.config_path, &before).unwrap();
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    std::fs::remove_dir_all(cfg.repo_root.join(".git/mock-instances")).unwrap();
    let again = Config::from_dir(&cfg.mock_dir);

    let finishing = plan(&again, &RESEARCH).unwrap().lines.join("\n");
    assert!(finishing.contains("did not record") && finishing.contains("[[data_ref]]"), "{finishing}");
    assert_eq!(run(&again, &RESEARCH), ExitCode::SUCCESS);
    let done = Config::from_dir(&cfg.mock_dir);
    assert_eq!(done.data_refs.len(), 1);
    assert_eq!(crate::data_refs::standing(&done), Ok(()));
    assert_eq!(crate::data_refs::run(&done, &["verify"]), ExitCode::SUCCESS);

    // the control: the same stop, and the branch's files are not the ref's any more
    std::fs::write(&cfg.config_path, &before).unwrap();
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    std::fs::remove_dir_all(cfg.repo_root.join(".git/mock-instances")).unwrap();
    write(&cfg, "research/note.md", "changed since\n");
    git(&cfg.repo_root, &["add", "-f", "--", "mock/research"]);
    git(&cfg.repo_root, &["commit", "-q", "-m", "changed since"]);
    let why = plan(&Config::from_dir(&cfg.mock_dir), &RESEARCH).unwrap_err().join("\n");
    assert!(why.contains("do not hold what the branch tracks"), "{why}");
}

#[test]
fn a_commit_of_a_data_ref_is_stopped_where_the_lints_refuse_the_tree() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    let cfg = Config::from_dir(&cfg.mock_dir);
    let tip = || cfg.data_refs[0].open(&cfg.repo_root).unwrap().local_tip().unwrap().unwrap();
    let before = tip();
    write(&cfg, "research/note.md", "a change the gates have a say on\n");
    let (refuse, pass) = (|_: &std::path::Path| false, |_: &std::path::Path| true);

    assert_eq!(crate::data_refs::run_gated(&cfg, &["commit"], &refuse), ExitCode::FAILURE);
    assert_eq!(tip(), before, "the lints refused and the ref moved");
    let changed = crate::data_refs::ref_files(&cfg).unwrap().changed;
    assert!(changed.contains("research/note.md"), "the change left the gates' view: {changed:?}");
    // a verb that writes no ref is not held up by them
    assert_eq!(crate::data_refs::run_gated(&cfg, &["status"], &refuse), ExitCode::SUCCESS);

    // the control: where they pass, the same call commits
    assert_eq!(crate::data_refs::run_gated(&cfg, &["commit"], &pass), ExitCode::SUCCESS);
    assert_ne!(tip(), before);
    assert!(crate::data_refs::ref_files(&cfg).unwrap().changed.is_empty());
}
