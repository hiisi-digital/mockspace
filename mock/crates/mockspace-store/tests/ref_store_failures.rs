//! The ref backend when a fetch or a push goes wrong: loud failures, refusals, the branch form, a shallow writer.
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

// --- a fetch that fails is never an empty store ------------------------------------------

#[test]
fn an_unreachable_remote_is_an_error_and_never_an_empty_store() {
    let w = World::new();
    let gone = w
        .dir
        .path()
        .join("nowhere.git")
        .to_string_lossy()
        .into_owned();
    let s = RefStore::new(w.clone_dir("a"), gone, RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(2));
    assert!(matches!(s.refresh(), Err(StoreError::Fetch { .. })));
    assert!(
        matches!(s.list(""), Err(StoreError::NotFetched { .. })),
        "a read after a failed fetch must not answer `empty`"
    );
    assert!(matches!(s.read("x"), Err(StoreError::NotFetched { .. })));
    assert!(matches!(
        s.apply(&Edit::new().put("x", "1"), "m", &Reapply),
        Err(StoreError::Fetch { .. })
    ));
}

#[test]
fn a_reachable_remote_with_no_ref_is_empty_and_says_so() {
    let w = World::new();
    let s = store(&w, "a");
    assert!(
        matches!(s.list(""), Err(StoreError::NotFetched { .. })),
        "before any fetch the store does not know"
    );
    assert_eq!(s.refresh().unwrap(), Freshness::Empty);
    assert_eq!(s.list("").unwrap(), Vec::<String>::new());
}

#[test]
fn a_ref_the_remote_lost_is_not_dropped_quietly() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    git(&w.bare, &["update-ref", "-d", MOCK_REF]);
    assert!(matches!(s.refresh(), Err(StoreError::RemoteLostRef { .. })));
    assert_eq!(
        s.read("a").unwrap().as_deref(),
        Some(&b"1"[..]),
        "the local copy is still there to restore from"
    );
}

#[test]
fn a_remote_that_went_away_after_a_fetch_is_loud_on_the_next_refresh() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    std::fs::rename(&w.bare, w.dir.path().join("moved.git")).unwrap();
    assert!(matches!(s.refresh(), Err(StoreError::Fetch { .. })));
}

// --- a push that is refused is final, not a race -----------------------------------------

#[test]
fn a_refusal_that_is_not_a_race_is_not_retried() {
    let w = World::new();
    let count = w.dir.path().join("pushes");
    w.install_hook(
        &w.bare,
        "pre-receive",
        &format!(
            "echo x >> '{}'\necho 'declined by policy' >&2\nexit 1",
            count.display()
        ),
    );
    let s = RefStore::new(w.clone_dir("a"), w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(5));
    let err = s
        .apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap_err();
    match err {
        StoreError::PushRefused {
            detail,
            ..
        } => assert!(detail.contains("declined by policy"), "{detail}"),
        other => panic!("{other}"),
    }
    assert_eq!(
        std::fs::read_to_string(&count).unwrap().lines().count(),
        1,
        "pushed once and not again"
    );
    assert!(w.tip(MOCK_REF).is_none());
}

// --- the branch fallback ---------------------------------------------------------------------

#[test]
fn the_branch_form_is_the_same_store_with_another_remote_ref() {
    let w = World::new();
    let pair = RefPair::branch_form(NS).unwrap();
    let a = RefStore::new(w.clone_dir("a"), w.remote(), pair.clone())
        .with_retry(RetryPolicy::immediate(3));
    a.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    assert_eq!(
        git(&w.bare, &["branch", "--list", "mock/registry/probe"]).trim(),
        "mock/registry/probe",
        "a branch on the remote"
    );
    assert!(
        w.tip(MOCK_REF).is_none(),
        "and nothing under refs/mock there"
    );

    let b_dir = w.clone_dir("b");
    let b = RefStore::new(&b_dir, w.remote(), pair).with_retry(RetryPolicy::immediate(3));
    assert!(matches!(b.refresh().unwrap(), Freshness::Fetched { .. }));
    assert_eq!(b.read("a").unwrap().as_deref(), Some(&b"1"[..]));
    // the local side stays a private ref in the clone, not a branch of it
    assert_eq!(git(&b_dir, &["branch", "--list"]), "");
}

// --- a shallow clone ----------------------------------------------------------------------------

#[test]
fn a_shallow_writer_adds_to_a_long_history() {
    let w = World::new();
    let long = store(&w, "long");
    for i in 0 .. 5 {
        long.apply(
            &Edit::new().put(format!("f{i}"), "x"),
            &format!("c{i}"),
            &Reapply,
        )
        .unwrap();
    }
    let repo = w.clone_dir("shallow");
    let shallow = RefStore::new(&repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(3))
        .with_depth(1);
    shallow.refresh().unwrap();
    assert_eq!(
        git(&repo, &["rev-list", "--count", MOCK_REF]),
        "1",
        "one commit of history locally"
    );
    shallow
        .apply(&Edit::new().put("late", "y"), "late", &Reapply)
        .unwrap();
    assert_eq!(
        w.commits(MOCK_REF),
        6,
        "the remote's history is whole and the new commit sits on it"
    );
    assert_eq!(shallow.list("").unwrap().len(), 6);
}

#[test]
fn a_remote_whose_history_was_rewritten_is_not_followed() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    let before = s.local_tip().unwrap().unwrap();
    // the remote ref now points at an unrelated root commit
    let blob = git(&w.bare, &["hash-object", "-w", "--stdin"]);
    let _ = blob;
    let orphan = {
        let env_git = |args: &[&str]| common::git(&w.bare, args);
        let tree = env_git(&["hash-object", "-t", "tree", "-w", "/dev/null"]);
        env_git(&["commit-tree", &tree, "-m", "orphan"])
    };
    git(&w.bare, &["update-ref", MOCK_REF, &orphan]);
    let err = s.refresh().unwrap_err();
    assert!(matches!(err, StoreError::RemoteRewrote { .. }), "{err}");
    assert_eq!(
        s.local_tip().unwrap().as_deref(),
        Some(before.as_str()),
        "the local ref is put back"
    );
    assert_eq!(
        s.read("a").unwrap().as_deref(),
        Some(&b"1"[..]),
        "and what was read still reads"
    );
}
