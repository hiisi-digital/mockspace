//! The merge drivers are registered in the user's git config as well as the clone's, and none of their
//! commands names a path of one clone.

use std::fs;
use std::path::Path;
use std::process::Command;

use super::*;

const ALL: [&str; 5] = [
    "mockspace-generated",
    "mockspace-registry",
    "mockspace-round",
    "mockspace-test-history",
    "mockspace-append",
];

fn git(dir: &Path, envs: &[(&str, &Path)], args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.output().expect("git runs");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn user_config(file: &Path, key: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["config", "--file"])
        .arg(file)
        .arg(key)
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[test]
fn registering_in_the_user_config_registers_every_driver_and_a_second_run_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("gitconfig");
    let mut first = Vec::new();
    register_all_drivers(&ConfigTarget::User(Some(&file)), &mut first);
    for d in ALL {
        assert!(
            user_config(&file, &format!("merge.{d}.driver")).is_some(),
            "{d} is not registered; actions: {first:?}"
        );
    }
    assert_eq!(first.len(), ALL.len(), "{first:?}");
    let mut second = Vec::new();
    register_all_drivers(&ConfigTarget::User(Some(&file)), &mut second);
    assert!(second.is_empty(), "{second:?}");
}

#[test]
fn no_registered_command_names_a_path_of_the_clone_it_was_written_from() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("a-clone-with-a-distinctive-name");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &[], &["init", "-q", "-b", "main"]);
    let mut actions = Vec::new();
    register_all_drivers(&ConfigTarget::Clone(&repo), &mut actions);
    for d in ALL {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "--local", &format!("merge.{d}.driver")])
            .output()
            .unwrap();
        let command = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(!command.is_empty(), "{d} is not registered: {actions:?}");
        assert!(
            !command.contains("a-clone-with-a-distinctive-name"),
            "{d} names its clone: {command}"
        );
    }
}

/// A repository with `docs/x.md` changed on both sides, whose `mockspace-generated` driver is the registered
/// command, with the script it finds replaced by one writing STUB.
fn a_conflict_under_the_generated_driver(
    root: &Path,
    config: &Path,
    local_registration: bool,
) -> std::path::PathBuf {
    let repo = root.join("one");
    fs::create_dir_all(repo.join("docs")).unwrap();
    let env = [("GIT_CONFIG_GLOBAL", config)];
    git(&repo, &env, &["init", "-q", "-b", "main"]);
    fs::write(repo.join(".gitattributes"), "/docs/* merge=mockspace-generated\n").unwrap();
    fs::write(repo.join("docs/x.md"), "a\n").unwrap();
    git(&repo, &env, &["add", "-A"]);
    git(&repo, &env, &["commit", "-qm", "base"]);
    git(&repo, &env, &["checkout", "-qb", "side"]);
    fs::write(repo.join("docs/x.md"), "side\n").unwrap();
    git(&repo, &env, &["commit", "-qam", "side"]);
    git(&repo, &env, &["checkout", "-q", "main"]);
    fs::write(repo.join("docs/x.md"), "main\n").unwrap();
    git(&repo, &env, &["commit", "-qam", "main"]);
    if local_registration {
        let mut actions = Vec::new();
        register_driver(&ConfigTarget::Clone(&repo), &mut actions);
    }
    repo
}

fn stub_script(repo: &Path, label: &str) {
    let script = repo.join(".git/mockspace/merge-generated");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, format!("#!/bin/sh\necho {label} > \"$2\"\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_generated_driver_still_runs_in_a_copy_of_the_clone_it_was_registered_in() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("none");
    fs::write(&config, "").unwrap();
    let repo = a_conflict_under_the_generated_driver(tmp.path(), &config, true);
    stub_script(&repo, "ORIGINAL");
    let copy = tmp.path().join("two");
    let cp = Command::new("cp").arg("-r").arg(&repo).arg(&copy).status().unwrap();
    assert!(cp.success());
    stub_script(&copy, "COPY");
    git(&copy, &[("GIT_CONFIG_GLOBAL", config.as_path())], &["merge", "side"]);
    assert_eq!(
        fs::read_to_string(copy.join("docs/x.md")).unwrap().trim(),
        "COPY",
        "the registered command did not reach the script of the copy"
    );
}

#[test]
fn the_generated_driver_registered_for_the_user_runs_in_a_clone_that_never_registered_it() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("gitconfig");
    register_all_drivers(&ConfigTarget::User(Some(&config)), &mut Vec::new());
    let repo = a_conflict_under_the_generated_driver(tmp.path(), &config, false);
    stub_script(&repo, "STUB");
    git(&repo, &[("GIT_CONFIG_GLOBAL", config.as_path())], &["merge", "side"]);
    assert_eq!(fs::read_to_string(repo.join("docs/x.md")).unwrap().trim(), "STUB");
}

#[test]
fn the_user_registered_generated_driver_without_a_script_merges_as_git_would() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("gitconfig");
    register_all_drivers(&ConfigTarget::User(Some(&config)), &mut Vec::new());
    let repo = a_conflict_under_the_generated_driver(tmp.path(), &config, false);
    git(&repo, &[("GIT_CONFIG_GLOBAL", config.as_path())], &["merge", "side"]);
    let text = fs::read_to_string(repo.join("docs/x.md")).unwrap();
    assert!(text.contains("<<<<<<<") && text.contains("side") && text.contains("main"), "{text}");
}
