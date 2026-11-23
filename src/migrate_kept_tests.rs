//! Every step of a migration, and every file the setup of the instances writes in git's own
//! directory, keeps a copy of what stood there in the cache first (`mockspace_store::kept`). Each
//! test writes over one and asks the copies this process kept for the bytes that stood there.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::tests::{RESEARCH, consumer, git};
use super::*;
use crate::config::Config;
use crate::registry::test_world::World;

/// The bytes of every file copy this process kept of `path`.
fn kept(path: &Path) -> Vec<Vec<u8>> {
    mockspace_store::kept::copies_of(path).unwrap().iter().filter(|p| p.is_file()).map(|p| std::fs::read(p).unwrap()).collect()
}

fn git_path(cfg: &Config, what: &str) -> PathBuf {
    PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-path", what]).trim())
}

// Mutation: drop the copy of `AppendConfig`, of `Write`, or of either side of `Rename` in
// `carry_out`; the bytes the step wrote over are then nowhere.
#[test]
fn every_step_of_a_migration_keeps_what_it_writes_over() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    let config = std::fs::read(&cfg.config_path).unwrap();
    carry_out(&cfg, &Step::AppendConfig("# appended\n".into())).unwrap();
    assert!(kept(&cfg.config_path).contains(&config), "the config as it was is kept");

    let note = cfg.mock_dir.join("research/note.md");
    carry_out(&cfg, &Step::Write { path: "mock/research/note.md".into(), text: "rewritten\n".into() }).unwrap();
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "rewritten\n", "the control: the step wrote");
    assert!(kept(&note).contains(&b"findings\n".to_vec()), "the file written over is kept");

    let (from, to) = (cfg.mock_dir.join("research/deep/er.md"), cfg.mock_dir.join("research/deep/new.md"));
    std::fs::write(&to, "stood in the way\n").unwrap();
    carry_out(&cfg, &Step::Rename { from: "mock/research/deep/er.md".into(), to: "mock/research/deep/new.md".into() }).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "more\n", "the control: the step renamed over it");
    assert!(kept(&from).contains(&b"more\n".to_vec()), "the file renamed is kept at its old path");
    assert!(kept(&to).contains(&b"stood in the way\n".to_vec()), "the file renamed over is kept");
}

// Mutation: drop the copy before `instantiate` writes the hook, the monitor, the sparse-checkout
// file, the config or the record of the declarations (`data_refs/hook.rs`, `monitor.rs`,
// `sparse.rs`); what the person or an earlier engine left there is then nowhere.
#[test]
fn the_setup_of_the_instances_keeps_every_file_of_git_it_writes_over() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    let cfg = Config::from_dir(&cfg.mock_dir);
    let common = PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim());
    let files = [
        common.join("hooks/reference-transaction"),
        common.join("hooks/post-checkout"),
        common.join("hooks/mockspace-fsmonitor"),
        git_path(&cfg, "info/sparse-checkout"),
        git_path(&cfg, "mock-data-refs.json"),
    ];
    for f in &files {
        assert!(f.is_file(), "the control: {} is there", f.display());
    }
    // what an earlier engine left: each managed file with other bytes, and a setting taken out
    let hook = "#!/bin/sh\n# mockspace data refs: copies an edited instance file before git writes over it (managed, do not edit)\nexit 0\n";
    let stale: Vec<(PathBuf, Vec<u8>)> = files
        .iter()
        .map(|f| {
            let text = match f.file_name().and_then(|n| n.to_str()) {
                Some("reference-transaction") => hook.to_string(),
                Some("post-checkout") => "#!/bin/sh\n# mockspace data refs: copies the files of each instance after a checkout moved HEAD (managed, do not edit)\nexit 0\n".into(),
                Some("mockspace-fsmonitor") => "#!/bin/sh\n# an older monitor\n".into(),
                Some("sparse-checkout") => "/*\n".into(),
                _ => "[]".into(),
            };
            std::fs::write(f, &text).unwrap();
            (f.clone(), text.into_bytes())
        })
        .collect();
    git(&cfg.repo_root, &["config", "--unset", "sparse.expectFilesOutsideOfPatterns"]);
    let config = std::fs::read(common.join("config")).unwrap();
    // the instance is there already, which `instantiate` says, and it sets up the worktree all the same
    let _ = crate::data_refs::run(&cfg, &["instantiate"]);
    assert_eq!(crate::data_refs::run(&cfg, &["verify"]), ExitCode::SUCCESS, "the control: the setup is whole again");
    for (f, was) in &stale {
        assert_ne!(&std::fs::read(f).unwrap(), was, "the control: {} was written", f.display());
        assert!(kept(f).contains(was), "{} is kept as it was", f.display());
    }
    assert!(kept(&common.join("config")).contains(&config), "the config is kept before a setting changes");

    // a snapshot where the index tracks no file of an instance takes the monitor's file out, and
    // keeps it first
    let tracked = git_path(&cfg, "mock-data-refs.tracked");
    std::fs::write(&tracked, "left by an earlier snapshot\n").unwrap();
    assert_eq!(crate::data_refs::run(&cfg, &["snapshot"]), ExitCode::SUCCESS);
    assert!(!tracked.exists(), "the control: the index tracks no file of an instance here");
    assert!(kept(&tracked).contains(&b"left by an earlier snapshot\n".to_vec()), "the monitor's file is kept before it is removed");

    // with no ref declared any more, the record of the declarations is removed, and kept first
    let record = git_path(&cfg, "mock-data-refs.json");
    let held = std::fs::read(&record).unwrap();
    let text = std::fs::read_to_string(&cfg.config_path).unwrap();
    std::fs::write(&cfg.config_path, text.split("[[data_ref]]").next().unwrap()).unwrap();
    let bare = Config::from_dir(&cfg.mock_dir);
    assert!(bare.data_refs.is_empty(), "the control: nothing is declared");
    assert_eq!(crate::data_refs::run(&bare, &["instantiate"]), ExitCode::SUCCESS);
    assert!(!record.exists(), "the control: the record is gone");
    assert!(kept(&record).contains(&held), "the record is kept before it is removed");
}

// Mutation: let the sparse-checkout file read as empty where it cannot be read; the setup then
// writes the block over the person's lines it never read.
#[test]
fn a_sparse_checkout_file_that_is_not_text_is_refused_and_left_as_it_is() {
    let w = World::new();
    let cfg = consumer(&w, "a");
    assert_eq!(run(&cfg, &RESEARCH), ExitCode::SUCCESS);
    let cfg = Config::from_dir(&cfg.mock_dir);
    let file = git_path(&cfg, "info/sparse-checkout");
    let odd = b"/*\n/caf\xe9/\n".to_vec();
    std::fs::write(&file, &odd).unwrap();
    for verb in ["instantiate", "verify"] {
        assert_eq!(crate::data_refs::run(&cfg, &[verb]), ExitCode::FAILURE, "{verb} refuses");
        assert_eq!(std::fs::read(&file).unwrap(), odd, "{verb} left the file as it was");
    }
    // the control: made readable, the same call writes the block after the person's lines
    std::fs::write(&file, "/*\n/cafe/\n").unwrap();
    let _ = crate::data_refs::run(&cfg, &["instantiate"]);
    assert_eq!(crate::data_refs::run(&cfg, &["verify"]), ExitCode::SUCCESS);
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("/*\n/cafe/\n"));
}
