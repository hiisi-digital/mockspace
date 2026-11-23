//! Every mock tree on an orphan custom ref: the backup ref of the last tip a clone saw and the
//! refusal of a tip that does not descend from it, the push through a relay the clone names, and
//! the shadow line as ordinary local branches mapped by refspec onto `refs/mock/+/*`. Real git
//! and a real bare remote throughout.
#[path = "../../../../tests/common/ambient.rs"]
mod ambient;

mod common;

use common::{MOCK_REF, NS, World, git, store, try_git};
use mockspace_store::{Edit, PUSH_URL_KEY, Reader, Reapply, RefPair, RefStore, RetryPolicy, Store, StoreError};

const SEEN: &str = "refs/mock-seen/registry/probe";

/// An unrelated root commit, put at the remote's ref as a rewrite of its history.
fn rewrite_remote(w: &World) -> String {
    let tree = git(&w.bare, &["hash-object", "-t", "tree", "-w", "/dev/null"]);
    let orphan = git(&w.bare, &["commit-tree", &tree, "-m", "rewritten"]);
    git(&w.bare, &["update-ref", MOCK_REF, &orphan]);
    orphan
}

#[test]
fn each_fetch_and_each_write_moves_the_backup_ref_to_the_tip_it_saw() {
    let w = World::new();
    let a = store(&w, "a");
    let one = a.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap().commit.unwrap();
    let repo_a = w.dir.path().join("a");
    assert_eq!(git(&repo_a, &["rev-parse", SEEN]), one, "the write is the last tip seen");

    let b = RefStore::new(w.clone_dir("b"), w.remote(), RefPair::for_namespace(NS).unwrap());
    b.refresh().unwrap();
    assert_eq!(git(&w.dir.path().join("b"), &["rev-parse", SEEN]), one, "and so is a fetch");

    let two = a.apply(&Edit::new().put("a", "2"), "two", &Reapply).unwrap().commit.unwrap();
    b.refresh().unwrap();
    assert_eq!(git(&w.dir.path().join("b"), &["rev-parse", SEEN]), two, "a tip that descends moves it on");
}

#[test]
fn a_tip_that_does_not_descend_from_the_last_seen_is_refused_even_after_a_plain_forced_fetch() {
    let w = World::new();
    let a = store(&w, "a");
    let one = a.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap().commit.unwrap();
    let repo = w.dir.path().join("a");
    let orphan = rewrite_remote(&w);
    // plain git, which knows nothing of the store, forces the local mirror onto the rewrite
    git(&repo, &["fetch", "-q", &w.remote(), &format!("+{MOCK_REF}:{MOCK_REF}")]);
    assert_eq!(git(&repo, &["rev-parse", MOCK_REF]), orphan, "control: git followed the rewrite");

    // a store opened afresh on the clone knows the last tip from the backup ref alone
    let fresh = RefStore::new(&repo, w.remote(), RefPair::for_namespace(NS).unwrap());
    let err = fresh.refresh().unwrap_err();
    assert!(
        matches!(&err, StoreError::RemoteRewrote { local_tip, remote_tip, .. } if *local_tip == one && *remote_tip == orphan),
        "{err}"
    );
    assert_eq!(git(&repo, &["rev-parse", MOCK_REF]), one, "the local ref is put back on the last tip seen");
    assert_eq!(git(&repo, &["rev-parse", SEEN]), one, "and the backup ref is not moved");
    assert!(fresh.read("a").is_err(), "the store holds no tip, so nothing is read from the refused one");
}

#[test]
fn a_remote_that_lost_the_tree_is_refused_by_a_clone_whose_local_ref_is_gone() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap();
    let repo = w.dir.path().join("a");
    git(&w.bare, &["update-ref", "-d", MOCK_REF]);
    git(&repo, &["update-ref", "-d", MOCK_REF]);
    let fresh = RefStore::new(&repo, w.remote(), RefPair::for_namespace(NS).unwrap());
    let err = fresh.refresh().unwrap_err();
    assert!(matches!(err, StoreError::RemoteLostRef { .. }), "{err}");
}

#[test]
fn a_write_is_pushed_to_the_relay_the_clone_names_and_read_back_from_the_remote() {
    let w = World::new();
    let repo = w.clone_dir("a");
    let s = RefStore::new(&repo, w.remote(), RefPair::for_namespace(NS).unwrap()).with_retry(RetryPolicy::immediate(2));
    assert_eq!(s.push_target().unwrap(), w.remote(), "control: no relay, the remote it fetches from");

    let missing = w.dir.path().join("no-relay.git").to_string_lossy().into_owned();
    git(&repo, &["config", PUSH_URL_KEY, &missing]);
    let err = s.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap_err();
    assert!(matches!(&err, StoreError::PushRefused { remote, .. } if *remote == missing), "{err}");
    assert!(w.tip(MOCK_REF).is_none(), "the push went to the relay and nowhere else");

    // the same remote by another address stands in for a relay that writes through to it
    let relay = format!("file://{}", w.remote());
    git(&repo, &["config", PUSH_URL_KEY, &relay]);
    let landed = s.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap();
    assert!(landed.confirmed, "the remote it fetches from holds what the relay took");
    assert_eq!(w.tip(MOCK_REF), landed.commit);
}

#[test]
fn a_shadow_branch_is_a_local_branch_that_a_plain_push_puts_on_a_custom_ref() {
    let w = World::new();
    let a = w.clone_dir("a");
    git(&a, &["remote", "add", "origin", &w.remote()]);
    git(&a, &["config", "--add", "remote.origin.push", &RefPair::shadow_push_refspec()]);
    git(&a, &["config", "--add", "remote.origin.fetch", &RefPair::shadow_fetch_refspec("origin")]);
    let pair = RefPair::shadow("dev").unwrap();
    git(&a, &["commit", "-q", "--allow-empty", "-m", "shadow root"]);
    git(&a, &["branch", "-q", "-m", "+/dev"]);
    assert_eq!(git(&a, &["branch", "--show-current"]), "+/dev", "git's own tooling takes it as a branch");
    git(&a, &["push", "-q", "origin"]);
    let tip = git(&a, &["rev-parse", pair.local()]);
    assert_eq!(w.tip(pair.remote()).as_deref(), Some(tip.as_str()));
    assert_eq!(git(&w.bare, &["for-each-ref", "--format=%(refname)", "refs/heads/"]), "", "nothing under refs/heads");

    let b = w.clone_dir("b");
    git(&b, &["remote", "add", "origin", &w.remote()]);
    git(&b, &["config", "--add", "remote.origin.fetch", &RefPair::shadow_fetch_refspec("origin")]);
    git(&b, &["fetch", "-q", "origin"]);
    assert_eq!(git(&b, &["rev-parse", "refs/remotes/origin/+/dev"]), tip, "fetched beside the branches, never over one");
    assert!(try_git(&b, &["rev-parse", "-q", "--verify", pair.local()]).is_err());
}
