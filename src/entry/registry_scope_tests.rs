use std::path::PathBuf;
use std::process::Command;

use super::registry_scope::*;
use crate::LintMode;
use crate::registry::RegistryFinding;

fn finding(kind: &'static str, source: Option<&str>) -> RegistryFinding {
    RegistryFinding {
        kind,
        message: "m".into(),
        source: source.map(PathBuf::from),
    }
}

#[test]
fn a_dangling_reference_in_a_file_the_commit_does_not_change_is_a_warning() {
    let f = finding("unknown-row-reference", Some("/r/a.toml"));
    let changed = Changed::Staged(vec![PathBuf::from("/r/b.toml")]);
    assert_eq!(verdict(&f, &changed), Verdict::Warning);
    assert_eq!(report([f], &changed), (0, 1));
}

#[test]
fn the_same_reference_in_a_file_the_commit_stages_is_an_error() {
    let f = finding("unknown-row-reference", Some("/r/a.toml"));
    let changed = Changed::Staged(vec![PathBuf::from("/r/a.toml")]);
    assert_eq!(verdict(&f, &changed), Verdict::Error);
    assert_eq!(report([f], &changed), (1, 0));
}

#[test]
fn a_plain_run_reports_a_dangling_reference_and_gates_nothing() {
    let f = finding("unknown-row-reference", Some("/r/a.toml"));
    assert_eq!(verdict(&f, &Changed::NotCommit), Verdict::Warning);
    assert_eq!(report([f], &Changed::NotCommit), (0, 1));
}

#[test]
fn a_finding_with_no_source_keeps_failing_in_every_mode() {
    let f = finding("duplicate-id", None);
    for changed in [Changed::NotCommit, Changed::Unknown, Changed::Staged(vec![])] {
        assert_eq!(verdict(&f, &changed), Verdict::Error);
    }
}

#[test]
fn a_commit_whose_staged_files_are_unknown_gates_every_finding() {
    let f = finding("unknown-row-reference", Some("/r/a.toml"));
    assert_eq!(verdict(&f, &Changed::Unknown), Verdict::Error);
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let s = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
    assert!(s.success(), "git {args:?}");
}

#[test]
fn the_commit_scope_is_what_the_index_changes() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("a.toml"), "x").unwrap();
    std::fs::write(root.join("b.toml"), "x").unwrap();
    git(root, &["add", "a.toml"]);
    let Changed::Staged(paths) = Changed::of_run(LintMode::Commit, root) else {
        panic!("a commit run reads the index");
    };
    assert_eq!(paths, vec![root.join("a.toml")]);
    // the control: only the commit gate reads the index
    assert!(matches!(Changed::of_run(LintMode::Build, root), Changed::NotCommit));
}
