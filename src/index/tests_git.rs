use std::path::Path;
use std::process::Command;

use mockspace_lint_rules::{IndexQuery, RepoIndex};

use super::{Index, Source};
use crate::index::stamp::days_from_civil;
use mockspace_lint_rules::DocClass;

fn git(root: &Path, when: &str, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", when)
        .env("GIT_COMMITTER_DATE", when)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed");
}

fn commit(root: &Path, when: &str, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, body).unwrap();
    git(root, when, &["add", rel]);
    git(root, when, &["commit", "-q", "-m", "x"]);
}

fn source(root: &Path) -> Vec<Source> {
    vec![Source {
        dir:   root.join("research"),
        shown: "research".into(),
        class: DocClass::Research,
        about: None,
    }]
}

fn ask(idx: &Index, today: i64) -> Vec<(String, Option<i64>)> {
    let q = IndexQuery { text: "saturation curve", expand: &[], limit: 5, today };
    idx.search(&q).hits.into_iter().map(|h| (h.path, h.day)).collect()
}

#[test]
fn an_unstamped_note_is_dated_by_the_commit_that_added_it() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), "2026-01-01T00:00:00Z", &["init", "-q"]);
    commit(t.path(), "2026-03-01T12:00:00Z", "research/plain.md", "saturation curve of air");
    commit(t.path(), "2026-06-01T12:00:00Z", "research/plain.md", "saturation curve of air, more");
    commit(
        t.path(),
        "2026-02-01T12:00:00Z",
        "research/202604100000_stamped.md",
        "saturation curve of air",
    );

    let mut idx = Index::in_memory().unwrap();
    idx.refresh_dated(&source(t.path()), Some(t.path())).unwrap();
    let got = ask(&idx, 0);
    let day = |p: &str| got.iter().find(|(h, _)| h.ends_with(p)).unwrap().1;
    assert_eq!(day("plain.md"), Some(days_from_civil(2026, 3, 1)), "first added, not last changed");
    // THE CONTROL: a name's own stamp wins over the commit date.
    assert_eq!(day("stamped.md"), Some(days_from_civil(2026, 4, 10)));
}

#[test]
fn without_a_repository_an_unstamped_note_has_no_day() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), "2026-01-01T00:00:00Z", &["init", "-q"]);
    commit(t.path(), "2026-03-01T12:00:00Z", "research/plain.md", "saturation curve of air");
    let mut idx = Index::in_memory().unwrap();
    idx.refresh(&source(t.path())).unwrap();
    assert_eq!(ask(&idx, 0)[0].1, None);
    assert_eq!(idx.changed_day("research/plain.md"), None);
}

#[test]
fn the_day_a_path_last_changed_is_asked_of_git() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), "2026-01-01T00:00:00Z", &["init", "-q"]);
    commit(t.path(), "2026-03-01T12:00:00Z", "mock/registry/a.toml", "one");
    commit(t.path(), "2026-05-09T12:00:00Z", "mock/registry/a.toml", "two");
    let mut idx = Index::in_memory().unwrap();
    idx.refresh_dated(&source(t.path()), Some(t.path())).unwrap();
    assert_eq!(idx.changed_day("mock/registry/a.toml"), Some(days_from_civil(2026, 5, 9)));
    assert_eq!(idx.changed_day("mock/registry/none.toml"), None);
}

#[test]
fn a_shallow_clone_is_not_dated_and_says_so() {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), "2026-01-01T00:00:00Z", &["init", "-q"]);
    commit(t.path(), "2026-03-01T12:00:00Z", "research/plain.md", "saturation curve of air");
    commit(t.path(), "2026-03-02T12:00:00Z", "research/other.md", "something else");
    let into = tempfile::tempdir().unwrap();
    let dest = into.path().join("c");
    assert!(
        Command::new("git")
            .args(["clone", "-q", "--depth", "1"])
            .arg(format!("file://{}", t.path().display()))
            .arg(&dest)
            .status()
            .unwrap()
            .success()
    );
    let mut idx = Index::in_memory().unwrap();
    idx.refresh_dated(&source(&dest), Some(&dest)).unwrap();
    let q = IndexQuery { text: "saturation curve", expand: &[], limit: 5, today: 0 };
    let r = idx.search(&q);
    assert_eq!(r.hits[0].day, None);
    assert!(r.caveats.iter().any(|c| c.contains("shallow")), "{:?}", r.caveats);
}
