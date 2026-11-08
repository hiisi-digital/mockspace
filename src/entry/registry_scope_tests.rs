use std::path::{Path, PathBuf};
use std::process::Command;

use super::registry_scope::*;
use crate::LintMode;
use crate::registry::{GateScope, RegistryFinding, RegistryNamespace};

fn finding(kind: &'static str, gate: GateScope) -> RegistryFinding {
    RegistryFinding {
        kind,
        message: "m".into(),
        // Where it prints from. Set on every finding here, to show it decides nothing.
        source: Some(PathBuf::from("/r/a.toml")),
        gate,
    }
}

fn when_changed(file: &str) -> GateScope {
    GateScope::WhenChanged(PathBuf::from(file))
}

fn files(paths: &[&str]) -> Changed {
    Changed::Files(paths.iter().map(PathBuf::from).collect())
}

/// Every scope against every kind of run. The rows that must fail are the `Error` ones for
/// `WhenChanged`, and every `Always` row: a duplicate that names a file nobody changed still
/// gates.
#[test]
fn the_verdict_is_decided_by_the_scope_and_the_changed_files() {
    use Verdict::{Error, Warning};
    let runs: [(&str, Changed); 5] = [
        ("a plain run", Changed::NotCommit),
        ("the file changed", files(&["/r/a.toml"])),
        ("another file changed", files(&["/r/b.toml"])),
        ("nothing changed", files(&[])),
        ("git could not say", Changed::Unknown),
    ];
    let expected: [(GateScope, [Verdict; 5]); 2] = [
        (when_changed("/r/a.toml"), [
            Warning, Error, Warning, Warning, Error,
        ]),
        (GateScope::Always, [Error, Error, Error, Error, Error]),
    ];
    for (scope, verdicts) in expected {
        for ((what, changed), want) in runs.iter().zip(verdicts) {
            let f = finding("k", scope.clone());
            assert_eq!(verdict(&f, changed), want, "{scope:?} on {what}");
        }
    }
}

#[test]
fn report_counts_the_gated_and_the_demoted() {
    let fs = || {
        vec![
            finding("unknown-row-reference", when_changed("/r/a.toml")),
            finding("unknown-row-reference", when_changed("/r/b.toml")),
            finding("duplicate-id", GateScope::Always),
        ]
    };
    assert_eq!(report(fs(), &files(&["/r/a.toml"])), (2, 1));
    assert_eq!(report(fs(), &Changed::NotCommit), (1, 2));
    assert_eq!(report(fs(), &Changed::Unknown), (3, 0));
}

fn spike() -> RegistryNamespace {
    RegistryNamespace {
        key:         "spike".into(),
        title:       None,
        description: None,
        value_field: None,
        group_by:    None,
        fields:      vec![],
    }
}

/// The finding `validate` makes for a slug declared in two files, with only the second file
/// staged. It names the first file, so a scope inferred from `source` would warn here.
#[test]
fn a_duplicate_with_only_its_second_file_staged_still_gates() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("registry");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.toml"), "[[spike]]\nid = \"one\"\n").unwrap();
    std::fs::write(dir.join("b.toml"), "[[spike]]\nid = \"one\"\n").unwrap();
    let nss = [spike()];
    let reg = crate::registry::load_registry_in_dir(d.path(), &nss).unwrap();
    let found = crate::registry::validate(&nss, &reg);
    let dup: Vec<_> = found
        .into_iter()
        .filter(|f| f.kind == "duplicate-id")
        .collect();
    assert_eq!(dup.len(), 1, "{dup:?}");
    let second = Changed::Files(vec![dir.join("b.toml")]);
    assert_eq!(verdict(&dup[0], &second), Verdict::Error);
    assert_eq!(verdict(&dup[0], &Changed::NotCommit), Verdict::Error);
    assert_eq!(report(dup, &Changed::Files(vec![])), (1, 0));
}

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(s.success(), "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q"]);
    d
}

#[test]
fn the_commit_scope_is_what_the_index_changes() {
    let d = repo();
    let root = d.path();
    std::fs::write(root.join("a.toml"), "x").unwrap();
    std::fs::write(root.join("b.toml"), "x").unwrap();
    git(root, &["add", "a.toml"]);
    let Changed::Files(paths) = Changed::of_run(LintMode::Commit, root) else {
        panic!("a commit run reads the index");
    };
    assert_eq!(paths, vec![root.join("a.toml")]);
    // the control: a plain run reads nothing
    assert!(matches!(
        Changed::of_run(LintMode::Build, root),
        Changed::NotCommit
    ));
}

#[test]
fn the_push_scope_is_what_the_commits_not_on_a_remote_change() {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    let d = repo();
    let root = d.path();
    git(root, &[
        "remote",
        "add",
        "origin",
        remote.path().to_str().unwrap(),
    ]);
    std::fs::write(root.join("pushed.toml"), "x").unwrap();
    git(root, &["add", "pushed.toml"]);
    git(root, &["commit", "-qm", "first"]);
    git(root, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(root, &["fetch", "-q", "origin"]);
    std::fs::write(root.join("local.toml"), "x").unwrap();
    git(root, &["add", "local.toml"]);
    git(root, &["commit", "-qm", "second"]);
    let Changed::Files(paths) = Changed::of_run(LintMode::Push, root) else {
        panic!("a push run reads the unpushed commits");
    };
    // the file of the pushed commit is absent: it is already on a remote
    assert_eq!(paths, vec![root.join("local.toml")]);
}

#[test]
fn a_run_whose_files_git_cannot_name_gates_everything() {
    let d = tempfile::tempdir().unwrap();
    // a `.git` that is not a repository, as a fixture without `git init` has
    std::fs::create_dir(d.path().join(".git")).unwrap();
    for mode in [LintMode::Commit, LintMode::Push] {
        assert!(
            matches!(Changed::of_run(mode, d.path()), Changed::Unknown),
            "{mode:?}"
        );
    }
}

/// `git commit -a` commits a temporary index and leaves `.git/index` alone, so only the index the
/// hook was handed holds the edit. The control is the same call without it.
#[test]
fn the_commit_scope_reads_the_index_the_hook_was_given() {
    let d = repo();
    let root = d.path();
    std::fs::write(root.join("a.toml"), "x").unwrap();
    git(root, &["add", "a.toml"]);
    git(root, &["commit", "-qm", "base"]);
    std::fs::write(root.join("a.toml"), "edited").unwrap();
    std::fs::copy(root.join(".git/index"), root.join(".git/next-index")).unwrap();
    let s = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "-u"])
        .env("GIT_INDEX_FILE", root.join(".git/next-index"))
        .status()
        .unwrap();
    assert!(s.success());
    let temporary = Path::new(".git/next-index");
    let Changed::Files(with) = Changed::of_run_with_index(LintMode::Commit, root, Some(temporary))
    else {
        panic!("a commit run reads an index");
    };
    assert_eq!(with, vec![root.join("a.toml")]);
    let Changed::Files(without) = Changed::of_run_with_index(LintMode::Commit, root, None) else {
        panic!("a commit run reads an index");
    };
    assert!(
        without.is_empty(),
        "the default index holds no edit: {without:?}"
    );
}

/// A merge commit lists no file by default. What it brought in from a branch already on a remote
/// is its diff against the first parent.
#[test]
fn the_push_scope_sees_a_merge_of_a_branch_a_remote_has() {
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    let d = repo();
    let root = d.path();
    git(root, &[
        "remote",
        "add",
        "origin",
        remote.path().to_str().unwrap(),
    ]);
    std::fs::write(root.join("base.toml"), "x").unwrap();
    git(root, &["add", "base.toml"]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["branch", "-M", "dev"]);
    git(root, &["push", "-q", "origin", "dev"]);
    git(root, &["checkout", "-qb", "feature"]);
    std::fs::write(root.join("cites.toml"), "x").unwrap();
    git(root, &["add", "cites.toml"]);
    git(root, &["commit", "-qm", "feature"]);
    git(root, &["push", "-q", "origin", "feature"]);
    git(root, &["checkout", "-q", "dev"]);
    std::fs::write(root.join("other.toml"), "x").unwrap();
    git(root, &["add", "other.toml"]);
    git(root, &["commit", "-qm", "other"]);
    git(root, &["push", "-q", "origin", "dev"]);
    git(root, &["fetch", "-q", "origin"]);
    git(root, &["merge", "-q", "--no-ff", "-m", "merge", "feature"]);
    let Changed::Files(paths) = Changed::of_run(LintMode::Push, root) else {
        panic!("a push run reads the unpushed commits");
    };
    assert_eq!(paths, vec![root.join("cites.toml")]);
}
