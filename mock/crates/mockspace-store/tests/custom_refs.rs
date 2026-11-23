//! Every mock tree on an orphan custom ref: the backup ref of the last tip a clone saw and the
//! refusal of a tip that does not descend from it, the push through a relay the clone names, and
//! the shadow line as ordinary local branches mapped by refspec onto `refs/mock/+/*`. Real git
//! and a real bare remote throughout.
#[path = "../../../../tests/common/ambient.rs"]
mod ambient;

mod common;

use common::{MOCK_REF, NS, World, git, git_shim, inst_dir, read, seeded, store, try_git};
use mockspace_store::{Edit, Instance, PUSH_URL_KEY, Reader, Reapply, RefPair, RefStore, RetryPolicy, Store, StoreError};

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
    let one = landed.commit.unwrap();
    assert_eq!(git(&repo, &["rev-parse", SEEN]), one);

    // a relay that takes the push and never passes it on: a mirror of the remote, so the lease holds
    let lossy = w.dir.path().join("lossy.git");
    git(w.dir.path(), &["clone", "-q", "--mirror", &w.remote(), &lossy.to_string_lossy()]);
    git(&repo, &["config", PUSH_URL_KEY, &lossy.to_string_lossy()]);
    let err = s.apply(&Edit::new().put("a", "2"), "two", &Reapply).unwrap_err();
    let StoreError::Verify { commit: two, .. } = &err else {
        panic!("the read-back asks the remote, not the relay: {err}");
    };
    assert_eq!(git(&lossy, &["rev-parse", MOCK_REF]), *two, "control: the relay took it");
    assert_eq!(git(&repo, &["rev-parse", MOCK_REF]), one, "the local ref is not moved onto it");
    assert_eq!(w.tip(MOCK_REF).as_deref(), Some(one.as_str()), "the remote never had it");
    assert_eq!(git(&repo, &["rev-parse", SEEN]), one, "and the clone does not take it for seen");
    let fresh = RefStore::new(&repo, w.remote(), RefPair::for_namespace(NS).unwrap());
    fresh.refresh().unwrap();
    assert_eq!(fresh.read("a").unwrap().as_deref(), Some(&b"1"[..]), "the clone goes on from the remote's tip");
}

#[test]
fn a_first_write_a_lossy_relay_dropped_leaves_the_tree_unborn_here_and_writable() {
    let w = World::new();
    let repo = w.clone_dir("a");
    let lossy = w.dir.path().join("lossy.git");
    git(w.dir.path(), &["clone", "-q", "--mirror", &w.remote(), &lossy.to_string_lossy()]);
    git(&repo, &["config", PUSH_URL_KEY, &lossy.to_string_lossy()]);
    let s = RefStore::new(&repo, w.remote(), RefPair::for_namespace(NS).unwrap()).with_retry(RetryPolicy::immediate(2));
    let err = s.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap_err();
    assert!(matches!(err, StoreError::Verify { .. }), "{err}");
    assert!(try_git(&repo, &["rev-parse", "-q", "--verify", MOCK_REF]).is_err(), "no local ref names the lost commit");
    assert!(try_git(&repo, &["rev-parse", "-q", "--verify", SEEN]).is_err(), "nor the backup ref");

    // the next write, through a relay that passes it on, makes the tree
    git(&repo, &["config", PUSH_URL_KEY, &format!("file://{}", w.remote())]);
    let landed = s.apply(&Edit::new().put("a", "1"), "one", &Reapply).unwrap();
    assert!(landed.confirmed);
    assert_eq!(w.tip(MOCK_REF), landed.commit);
}

#[test]
fn an_instance_whose_write_was_not_confirmed_keeps_the_edit_on_disk() {
    let w = World::new();
    let a = seeded(&w);
    // b's push lands, and every fetch after it fails, so the remote cannot be asked about it
    let pushed = w.dir.path().join("pushed");
    let shim = git_shim(
        &w,
        &format!("    touch '{}'", pushed.display()),
        &format!("    if [ -e '{}' ]; then echo 'fatal: unable to access: could not resolve host' >&2; exit 128; fi", pushed.display()),
    );
    let b = RefStore::new(w.clone_dir("b"), w.remote(), RefPair::for_namespace(NS).unwrap())
        .with_retry(RetryPolicy::immediate(2))
        .with_git_program(&shim);
    b.refresh().unwrap();
    let (da, db) = (inst_dir(&w, "a", "registry"), inst_dir(&w, "b", "registry"));
    let (ia, ib) = (Instance::new(&a, &da).unwrap(), Instance::new(&b, &db).unwrap());
    ia.materialize().unwrap();
    ib.materialize().unwrap();
    // a moves the tip past b's base, so b's edit is rebased onto a tip b never had on disk
    std::fs::write(da.join("ticket/won.toml"), "id = \"won\"\n").unwrap();
    ia.apply("won", &Reapply).unwrap();
    std::fs::write(db.join("ticket/t1.toml"), "id = \"t1\"\nedited = true\n").unwrap();
    let applied = ib.apply("edit t1", &Reapply).unwrap();
    assert!(applied.changed && !applied.confirmed, "control: the push went out and was not confirmed");
    assert_eq!(read(&db, "ticket/t1.toml"), "id = \"t1\"\nedited = true\n", "the edit is still on disk");
    let repo_b = w.dir.path().join("b");
    assert_ne!(git(&repo_b, &["rev-parse", MOCK_REF]), applied.commit.clone().unwrap(), "the local ref did not move onto it");
    assert_eq!(w.file(MOCK_REF, "ticket/t1.toml").as_deref(), Some("id = \"t1\"\nedited = true"), "control: it landed");
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
