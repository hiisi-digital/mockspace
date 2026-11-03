//! `apply_from`: a write that says which tip its edit was built on, so that the hook is asked
//! whenever that tip is not the one on the remote, whatever the store has read since.
mod common;

use common::{Recording, World, store};
use mockspace_store::{Edit, Reapply, Store};

#[test]
fn an_edit_built_on_an_older_tip_goes_through_the_hook_after_the_store_refreshed() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "1\n"), "one", &Reapply)
        .unwrap();
    let old = a.local_tip().unwrap().unwrap();
    a.apply(&Edit::new().put("g", "2\n"), "two", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let hook = Recording::default();
    b.apply_from(Some(&old), &Edit::new().put("h", "3\n"), "three", &hook)
        .unwrap();
    assert_eq!(
        hook.tips.lock().unwrap().len(),
        1,
        "the hook was asked once"
    );
}

#[test]
fn an_edit_built_on_the_tip_that_is_there_does_not_ask_the_hook() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "1\n"), "one", &Reapply)
        .unwrap();
    let tip = a.local_tip().unwrap().unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let hook = Recording::default();
    b.apply_from(Some(&tip), &Edit::new().put("h", "3\n"), "three", &hook)
        .unwrap();
    assert!(
        hook.tips.lock().unwrap().is_empty(),
        "nothing moved, nothing to merge"
    );
}

#[test]
fn an_edit_built_on_an_empty_ref_goes_through_the_hook_where_the_ref_has_commits() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "1\n"), "one", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let hook = Recording::default();
    b.apply_from(None, &Edit::new().put("h", "3\n"), "three", &hook)
        .unwrap();
    assert_eq!(hook.tips.lock().unwrap().len(), 1);
}

#[test]
fn apply_is_apply_from_the_tip_the_store_last_saw() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "1\n"), "one", &Reapply)
        .unwrap();
    let hook = Recording::default();
    a.apply(&Edit::new().put("g", "2\n"), "two", &hook).unwrap();
    assert!(hook.tips.lock().unwrap().is_empty());
}
