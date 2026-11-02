//! The ref backend against real git and a real bare repository: history, the race,
//! a fetch that fails, a push that is refused, the branch fallback, a shallow clone.

mod common;

use common::{EMPTY_TREE, Recording, World, git};
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

const NS: &str = "registry/probe";
const MOCK_REF: &str = "refs/mock/registry/probe";

fn store(w: &World, name: &str) -> RefStore {
    RefStore::new(
        w.clone_dir(name),
        w.remote(),
        RefPair::ref_form(NS).unwrap(),
    )
    .with_retry(RetryPolicy::immediate(4))
}

// --- history: every write is a commit on the last one -------------------------------

#[test]
fn every_write_is_a_commit_on_the_tip() {
    let w = World::new();
    let s = store(&w, "a");
    let one = s
        .apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    let two = s
        .apply(&Edit::new().put("b", "2"), "two", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    assert_eq!(
        w.commits(MOCK_REF),
        2,
        "v2's write_round_ref would leave one parentless commit"
    );
    assert_eq!(git(&w.bare, &["rev-parse", &format!("{two}^")]), one);
    assert_eq!(w.tip(MOCK_REF).as_deref(), Some(two.as_str()));
    assert_eq!(git(&w.bare, &["log", "-1", "--format=%s", MOCK_REF]), "two");
}

#[test]
fn the_identity_is_v2s_unless_pinned() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    assert_eq!(
        git(&w.bare, &[
            "log",
            "-1",
            "--format=%an <%ae>|%cn <%ce>",
            MOCK_REF
        ]),
        "mockspace <noreply@mockspace.local>|mockspace <noreply@mockspace.local>"
    );
    let pinned =
        store(&w, "b").with_identity(Identity::pinned("O. R. Toimela", "ort@hiisi.digital"));
    pinned
        .apply(&Edit::new().put("b", "2"), "two", &Reapply)
        .unwrap();
    assert_eq!(
        git(&w.bare, &["log", "-1", "--format=%an <%ae>", MOCK_REF]),
        "O. R. Toimela <ort@hiisi.digital>"
    );
}

#[test]
fn an_edit_that_changes_nothing_writes_no_commit() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("a", "1"), "one", &Reapply)
        .unwrap();
    let r = s
        .apply(&Edit::new().put("a", "1"), "same", &Reapply)
        .unwrap();
    assert!(!r.changed);
    assert_eq!(w.commits(MOCK_REF), 1);
    // nor does an empty edit create a ref
    let w2 = World::new();
    let r = store(&w2, "a")
        .apply(&Edit::new().remove("x"), "nothing", &Reapply)
        .unwrap();
    assert!(!r.changed && r.commit.is_none());
    assert!(w2.tip(MOCK_REF).is_none());
}

// --- the race -----------------------------------------------------------------------

/// A pre-push hook in `repo` that, on its first run only, moves the remote's ref to `rival`:
/// the other writer wins between this writer's fetch and its push.
fn rival_wins_once(w: &World, repo: &std::path::Path, rival: &str) {
    let mark = w.dir.path().join("rival.done");
    w.install_hook(
        repo,
        "pre-push",
        &format!(
            "[ -e '{m}' ] && exit 0\ntouch '{m}'\ngit --git-dir='{b}' update-ref {r} {rival}",
            m = mark.display(),
            b = w.bare.display(),
            r = MOCK_REF
        ),
    );
}

#[test]
fn a_lost_race_is_rebased_through_the_hook_and_keeps_both_commits() {
    let w = World::new();
    let first = store(&w, "a")
        .apply(&Edit::new().put("one", "1"), "one", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    let rival = w.competing_commit(&first, "rival.txt", "r");

    let loser_repo = w.clone_dir("b");
    let loser = RefStore::new(&loser_repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(4));
    rival_wins_once(&w, &loser_repo, &rival);
    let hook = Recording::default();
    let applied = loser
        .apply(&Edit::new().put("mine.txt", "m"), "mine", &hook)
        .unwrap();

    assert_eq!(applied.attempts, 2, "lost once, won the second time");
    let tips = hook.tips.lock().unwrap();
    assert_eq!(tips.len(), 1, "the hook is asked once per lost race");
    assert_eq!(
        tips[0],
        ["one", "rival.txt"],
        "the hook saw the tip that won, not its own"
    );
    let tip = w.tip(MOCK_REF).unwrap();
    assert_eq!(Some(tip.as_str()), applied.commit.as_deref());
    assert_eq!(
        git(&w.bare, &["rev-parse", &format!("{tip}^")]),
        rival,
        "the rebased commit sits on the winner's"
    );
    for f in ["one", "rival.txt", "mine.txt"] {
        assert!(w.file(MOCK_REF, f).is_some(), "{f} was lost");
    }
    assert_eq!(w.commits(MOCK_REF), 3);
}

#[test]
fn a_hook_that_refuses_leaves_the_winner_in_place() {
    let w = World::new();
    let first = store(&w, "a")
        .apply(&Edit::new().put("one", "1"), "one", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    let rival = w.competing_commit(&first, "rival.txt", "r");
    let repo = w.clone_dir("b");
    let loser = RefStore::new(&repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(4));
    rival_wins_once(&w, &repo, &rival);

    let err = loser
        .apply(&Edit::new().put("mine.txt", "m"), "mine", &Refuse)
        .unwrap_err();
    assert!(matches!(err, StoreError::Merge { .. }), "{err}");
    assert_eq!(
        w.tip(MOCK_REF).as_deref(),
        Some(rival.as_str()),
        "the refused writer pushed nothing"
    );
    assert!(w.file(MOCK_REF, "mine.txt").is_none());
}

#[test]
fn a_hook_can_change_the_edit_to_fit_the_new_tip() {
    struct AppendToCounter;
    impl MergeHook for AppendToCounter {
        fn rebase(&self, tip: &dyn Reader, _ours: &Edit) -> Result<Edit, MergeRefusal> {
            let n: u32 = String::from_utf8(tip.read("counter").unwrap().unwrap())
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            Ok(Edit::new().put("counter", format!("{}\n", n + 1)))
        }
    }
    let w = World::new();
    let first = store(&w, "a")
        .apply(&Edit::new().put("counter", "1\n"), "one", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    let rival = w.competing_commit(&first, "counter", "2\n");
    let repo = w.clone_dir("b");
    let loser = RefStore::new(&repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(4));
    rival_wins_once(&w, &repo, &rival);
    loser
        .apply(&Edit::new().put("counter", "2\n"), "bump", &AppendToCounter)
        .unwrap();
    assert_eq!(
        w.file(MOCK_REF, "counter").as_deref(),
        Some("3"),
        "the writer's bump landed on the winner's value"
    );
}

#[test]
fn a_writer_that_keeps_losing_gives_up_loudly() {
    let w = World::new();
    let s = store(&w, "a");
    s.apply(&Edit::new().put("one", "1"), "one", &Reapply)
        .unwrap();
    let repo = w.clone_dir("b");
    let loser = RefStore::new(&repo, w.remote(), RefPair::ref_form(NS).unwrap())
        .with_retry(RetryPolicy::immediate(3));
    // every push is preceded by another writer moving the ref
    w.install_hook(
        &repo,
        "pre-push",
        &format!(
            "export GIT_AUTHOR_NAME=r GIT_AUTHOR_EMAIL=r@x GIT_COMMITTER_NAME=r GIT_COMMITTER_EMAIL=r@x\n\
             t=$(git --git-dir='{b}' rev-parse {r})\n\
             c=$(git --git-dir='{b}' commit-tree {e} -p $t -m moved)\n\
             git --git-dir='{b}' update-ref {r} $c",
            b = w.bare.display(),
            r = MOCK_REF,
            e = EMPTY_TREE
        ),
    );
    let err = loser
        .apply(&Edit::new().put("mine", "m"), "mine", &Reapply)
        .unwrap_err();
    assert!(
        matches!(err, StoreError::RaceLost {
            attempts: 3,
        }),
        "{err}"
    );
    assert!(w.file(MOCK_REF, "mine").is_none());
}

#[test]
fn many_writers_at_once_lose_nothing() {
    let w = World::new();
    let remote = w.remote();
    let repos: Vec<_> = (0 .. 4).map(|i| w.clone_dir(&format!("w{i}"))).collect();
    let handles: Vec<_> = repos
        .into_iter()
        .enumerate()
        .map(|(i, repo)| {
            let remote = remote.clone();
            std::thread::spawn(move || {
                let s = RefStore::new(repo, remote, RefPair::ref_form(NS).unwrap())
                    .with_retry(RetryPolicy::immediate(80));
                for n in 0 .. 3 {
                    s.apply(
                        &Edit::new().put(format!("w{i}/{n}"), format!("{i}-{n}")),
                        &format!("w{i} {n}"),
                        &Reapply,
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let files = git(&w.bare, &["ls-tree", "-r", "--name-only", MOCK_REF]);
    assert_eq!(files.lines().count(), 12, "a write was lost:\n{files}");
    assert_eq!(w.commits(MOCK_REF), 12, "one linear commit per write");
    assert_eq!(
        git(&w.bare, &["rev-list", "--merges", "--count", MOCK_REF]),
        "0"
    );
}

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
