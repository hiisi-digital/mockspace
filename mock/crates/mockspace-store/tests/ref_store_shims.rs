//! The ref backend against a stand-in `git` that drops connections: pushes that landed, were refused, or never reached the remote.
#![allow(unused_imports)]
mod common;

use common::{Bump, EMPTY_TREE, MOCK_REF, NS, Recording, World, git, git_shim, store};
use mockspace_store::{
    Edit,
    Freshness,
    Identity,
    MergeHook,
    MergeRefusal,
    Reader,
    Reapply,
    RefPair,
    RefStore,
    Refuse,
    RetryPolicy,
    Store,
    StoreError,
};

#[test]
fn a_write_after_a_stale_read_goes_through_the_hook() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    assert_eq!(b.read("counter").unwrap().as_deref(), Some(&b"1\n"[..]));
    a.apply(&Edit::new().put("counter", "2\n"), "two", &Reapply)
        .unwrap();
    // b computed 2 from the 1 it read; the remote is already at 2
    b.apply(&Edit::new().put("counter", "2\n"), "bump", &Bump)
        .unwrap();
    assert_eq!(
        w.file(MOCK_REF, "counter").as_deref(),
        Some("3"),
        "the bump landed on a value it never saw"
    );
}

#[test]
fn a_push_the_remote_took_but_reported_failed_is_not_applied_twice() {
    let w = World::new();
    store(&w, "a")
        .apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap();
    let shim = git_shim(
        &w,
        "    git \"$@\"; echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128",
        "",
    );
    let b = store(&w, "b").with_git_program(&shim);
    b.refresh().unwrap();
    let applied = b
        .apply(&Edit::new().put("counter", "2\n"), "bump", &Bump)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(
        w.file(MOCK_REF, "counter").as_deref(),
        Some("2"),
        "one bump, not two"
    );
    assert_eq!(w.commits(MOCK_REF), 2);
    assert_eq!(w.tip(MOCK_REF).as_deref(), applied.commit.as_deref());
}

#[test]
fn a_dropped_connection_that_changed_nothing_is_tried_again() {
    let w = World::new();
    let mark = w.dir.path().join("dropped");
    let shim = git_shim(
        &w,
        &format!(
            "    [ -e '{m}' ] || {{ touch '{m}'; echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128; }}",
            m = mark.display()
        ),
        "",
    );
    let s = store(&w, "a").with_git_program(&shim);
    let applied = s
        .apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    assert_eq!(applied.attempts, 2);
    assert_eq!(w.commits(MOCK_REF), 1);
}

#[test]
fn a_shallow_writer_overtaken_after_its_push_still_knows_it_landed() {
    let w = World::new();
    let long = store(&w, "long");
    for i in 0 .. 3 {
        long.apply(
            &Edit::new().put(format!("f{i}"), "x"),
            &format!("c{i}"),
            &Reapply,
        )
        .unwrap();
    }
    // after the first push is accepted, another writer commits on top at once
    let mark = w.dir.path().join("overtaken");
    w.install_hook(
        &w.bare,
        "post-receive",
        &format!(
            "[ -e '{m}' ] && exit 0\ntouch '{m}'\n\
             export GIT_AUTHOR_NAME=r GIT_AUTHOR_EMAIL=r@x GIT_COMMITTER_NAME=r GIT_COMMITTER_EMAIL=r@x\n\
             t=$(git rev-parse {r})\nc=$(git commit-tree {e} -p $t -m overtaken)\ngit update-ref {r} $c",
            m = mark.display(),
            r = MOCK_REF,
            e = EMPTY_TREE
        ),
    );
    let repo = w.clone_dir("shallow");
    let shallow = RefStore::new(&repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(3))
        .with_depth(1);
    shallow.refresh().unwrap();
    let applied = shallow
        .apply(&Edit::new().put("late", "y"), "late", &Reapply)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(
        w.commits(MOCK_REF),
        5,
        "three before, ours, the overtaking one"
    );
    assert_eq!(git(&w.bare, &["show", &format!("{MOCK_REF}~1:late")]), "y");
}

// --- landed pushes whose follow-up fetch also dropped ---

#[test]
fn a_push_that_landed_whose_follow_up_fetch_also_dropped_is_still_not_applied_twice() {
    let w = World::new();
    store(&w, "a")
        .apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap();
    let pushed = w.dir.path().join("pushed");
    let dropped = w.dir.path().join("dropped-fetch");
    let shim = git_shim(
        &w,
        &format!(
            "    git \"$@\"; touch '{}'; echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128",
            pushed.display()
        ),
        &format!(
            "    if [ -e '{p}' ] && [ ! -e '{d}' ]; then touch '{d}'; echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128; fi",
            p = pushed.display(),
            d = dropped.display()
        ),
    );
    let b = store(&w, "b").with_git_program(&shim);
    b.refresh().unwrap();
    let applied = b
        .apply(&Edit::new().put("counter", "2\n"), "bump", &Bump)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(
        w.file(MOCK_REF, "counter").as_deref(),
        Some("2"),
        "one bump, not two"
    );
    assert_eq!(w.commits(MOCK_REF), 2);
}

#[test]
fn an_accepted_push_whose_check_could_not_reach_the_remote_is_landed_and_unconfirmed() {
    let w = World::new();
    let pushed = w.dir.path().join("pushed");
    let shim = git_shim(
        &w,
        &format!(
            "    git \"$@\"; rc=$?; touch '{}'; exit $rc",
            pushed.display()
        ),
        &format!(
            "    if [ -e '{}' ]; then echo 'fatal: unable to access: could not resolve host' >&2; exit 128; fi",
            pushed.display()
        ),
    );
    let s = store(&w, "a").with_git_program(&shim);
    let applied = s
        .apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    assert!(
        applied.changed && !applied.confirmed,
        "landed, and says it could not be confirmed"
    );
    assert_eq!(w.commits(MOCK_REF), 1);
}

#[test]
fn a_push_that_keeps_dropping_is_reported_as_that_and_not_as_a_lost_race() {
    let w = World::new();
    let shim = git_shim(
        &w,
        "    echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128",
        "",
    );
    let s = RefStore::new(w.clone_dir("a"), w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(2))
        .with_git_program(&shim);
    let err = s
        .apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap_err();
    match err {
        StoreError::PushRefused {
            detail,
            ..
        } => assert!(detail.contains("hung up"), "{detail}"),
        other => panic!("{other}"),
    }
    assert!(w.tip(MOCK_REF).is_none());
}

#[test]
fn a_store_reads_what_it_saw_even_when_another_store_on_the_clone_moves_the_local_ref() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap();
    let repo = w.clone_dir("shared");
    let pair = || RefPair::ref_form(NS).unwrap();
    let reader = RefStore::new(&repo, w.remote(), pair()).with_retry(RetryPolicy::immediate(4));
    let other = RefStore::new(&repo, w.remote(), pair()).with_retry(RetryPolicy::immediate(4));
    reader.refresh().unwrap();
    assert_eq!(
        reader.read("counter").unwrap().as_deref(),
        Some(&b"1\n"[..])
    );
    a.apply(&Edit::new().put("counter", "2\n"), "two", &Reapply)
        .unwrap();
    other.refresh().unwrap();
    assert_eq!(
        reader.read("counter").unwrap().as_deref(),
        Some(&b"1\n"[..]),
        "what it read does not change under it"
    );
    // it computed 2 from the 1 it read, and the remote is at 2
    reader
        .apply(&Edit::new().put("counter", "2\n"), "bump", &Bump)
        .unwrap();
    assert_eq!(w.file(MOCK_REF, "counter").as_deref(), Some("3"));
}

// --- shallow refresh and unknown outcomes ---

#[test]
fn a_shallow_store_keeps_refreshing_after_another_writer_moves_the_remote() {
    let w = World::new();
    let long = store(&w, "long");
    for i in 0 .. 3 {
        long.apply(&Edit::new().put(format!("f{i}"), "x"), "c", &Reapply)
            .unwrap();
    }
    let shallow = RefStore::new(
        w.clone_dir("shallow"),
        w.remote(),
        RefPair::ref_form(NS).unwrap(),
    )
    .with_retry(RetryPolicy::immediate(2))
    .with_depth(1);
    shallow.refresh().unwrap();
    long.apply(&Edit::new().put("more", "y"), "more", &Reapply)
        .unwrap();
    shallow
        .refresh()
        .expect("a remote that only moved forward is not a rewrite");
    assert_eq!(shallow.read("more").unwrap().as_deref(), Some(&b"y"[..]));
    long.apply(&Edit::new().put("again", "z"), "again", &Reapply)
        .unwrap();
    let applied = shallow
        .apply(&Edit::new().put("mine", "m"), "mine", &Reapply)
        .unwrap();
    assert!(applied.changed);
    assert_eq!(w.commits(MOCK_REF), 6);
}

#[test]
fn a_push_that_landed_with_every_later_fetch_dropped_is_an_unknown_outcome_not_a_plain_failure() {
    let w = World::new();
    store(&w, "a")
        .apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap();
    let pushed = w.dir.path().join("pushed");
    let shim = git_shim(
        &w,
        &format!(
            "    git \"$@\"; touch '{}'; echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128",
            pushed.display()
        ),
        &format!(
            "    if [ -e '{}' ]; then echo 'fatal: the remote end hung up unexpectedly' >&2; exit 128; fi",
            pushed.display()
        ),
    );
    let b = RefStore::new(w.clone_dir("b"), w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(2))
        .with_git_program(&shim);
    b.refresh().unwrap();
    let err = b
        .apply(&Edit::new().put("counter", "2\n"), "bump", &Bump)
        .unwrap_err();
    match err {
        StoreError::OutcomeUnknown {
            commits,
            ..
        } => assert!(!commits.is_empty()),
        other => panic!("{other}"),
    }
    assert_eq!(
        w.file(MOCK_REF, "counter").as_deref(),
        Some("2"),
        "it did land"
    );
}
