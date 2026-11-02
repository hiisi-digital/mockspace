//! The contract of a store, run on every backend: the same cases, so no behaviour
//! is true of one backend only. A new backend adds one `#[test]` that calls `suite`.

mod common;

use std::collections::BTreeMap;

use common::World;
use mockspace_store::{
    DirStore,
    Edit,
    Freshness,
    Reader,
    Reapply,
    RefPair,
    RefStore,
    RetryPolicy,
    Store,
    StoreError,
};

fn suite(store: &dyn Store, first_refresh: Freshness) {
    // an empty store is empty, and the answer is a real one
    assert_eq!(store.refresh().unwrap(), first_refresh);
    assert_eq!(store.list("").unwrap(), Vec::<String>::new());
    assert_eq!(store.read("a/b.toml").unwrap(), None);

    // one edit, several files, nested
    let r = store
        .apply(
            &Edit::new()
                .put("a/b.toml", "x = 1")
                .put("a/c/d.toml", "y = 2")
                .put("top.txt", "t"),
            "first",
            &Reapply,
        )
        .unwrap();
    assert!(r.changed);
    assert_eq!(
        store.read("a/b.toml").unwrap().as_deref(),
        Some(&b"x = 1"[..])
    );
    assert_eq!(store.list("").unwrap(), [
        "a/b.toml",
        "a/c/d.toml",
        "top.txt"
    ]);
    assert_eq!(store.list("a").unwrap(), ["a/b.toml", "a/c/d.toml"]);
    assert_eq!(store.list("a/c").unwrap(), ["a/c/d.toml"]);
    assert_eq!(
        store.list("to").unwrap(),
        Vec::<String>::new(),
        "a prefix names whole components"
    );
    let snap: BTreeMap<_, _> = store.snapshot().unwrap();
    assert_eq!(snap.len(), 3);
    assert_eq!(snap["a/c/d.toml"], b"y = 2");

    // binary content survives byte for byte
    let bytes: Vec<u8> = (0 ..= 255).collect();
    store
        .apply(&Edit::new().put("bin", bytes.clone()), "bin", &Reapply)
        .unwrap();
    assert_eq!(store.read("bin").unwrap().unwrap(), bytes);

    // the same content again changes nothing
    let r = store
        .apply(&Edit::new().put("a/b.toml", "x = 1"), "again", &Reapply)
        .unwrap();
    assert!(!r.changed, "an edit that changes nothing writes nothing");
    let r = store.apply(&Edit::new(), "empty", &Reapply).unwrap();
    assert!(!r.changed);

    // a directory is not a file
    assert_eq!(store.read("a").unwrap(), None);

    // a file and a directory of one name, against what is already stored
    store
        .apply(
            &Edit::new().put("clash/file", "f").put("clash/deep/x", "x"),
            "clash",
            &Reapply,
        )
        .unwrap();
    for (path, what) in [("clash/file/under", "below a file"), ("clash/deep", "over a directory")] {
        let err = store
            .apply(&Edit::new().put(path, "boom"), "clash", &Reapply)
            .unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidPath { .. }),
            "{what}: {err}"
        );
    }
    assert_eq!(
        store.read("clash/file").unwrap().as_deref(),
        Some(&b"f"[..]),
        "the file under a refused put was lost"
    );
    assert_eq!(
        store.read("clash/deep/x").unwrap().as_deref(),
        Some(&b"x"[..])
    );
    // unless the same edit takes what is in the way
    store
        .apply(
            &Edit::new()
                .remove("clash/file")
                .put("clash/file/under", "u"),
            "swap",
            &Reapply,
        )
        .unwrap();
    assert_eq!(
        store.read("clash/file/under").unwrap().as_deref(),
        Some(&b"u"[..])
    );
    store
        .apply(
            &Edit::new()
                .remove("clash/file/under")
                .remove("clash/deep/x"),
            "clear",
            &Reapply,
        )
        .unwrap();

    // paths that look like git pathspecs are plain names
    for odd in [":(glob)x", "*.txt", ":x", "[a]"] {
        store
            .apply(&Edit::new().put(odd, odd), "odd", &Reapply)
            .unwrap();
        assert_eq!(
            store.read(odd).unwrap().as_deref(),
            Some(odd.as_bytes()),
            "{odd}"
        );
    }
    assert!(store.snapshot().unwrap().contains_key(":(glob)x"));
    store
        .apply(
            &Edit::new()
                .remove(":(glob)x")
                .remove("*.txt")
                .remove(":x")
                .remove("[a]"),
            "odd off",
            &Reapply,
        )
        .unwrap();

    // replace, remove, and remove what is not there
    store
        .apply(
            &Edit::new()
                .put("a/b.toml", "x = 2")
                .remove("top.txt")
                .remove("never/was"),
            "second",
            &Reapply,
        )
        .unwrap();
    assert_eq!(
        store.read("a/b.toml").unwrap().as_deref(),
        Some(&b"x = 2"[..])
    );
    assert_eq!(store.read("top.txt").unwrap(), None);
    assert_eq!(store.list("never").unwrap(), Vec::<String>::new());

    // a path that leaves the store is refused and nothing of the edit is written
    for bad in ["../escape", "/abs", "a//b", ".git/x", ""] {
        let err = store
            .apply(
                &Edit::new().put("ok.txt", "1").put(bad, "2"),
                "bad",
                &Reapply,
            )
            .unwrap_err();
        assert!(
            matches!(err, StoreError::InvalidPath { .. }),
            "{bad:?}: {err}"
        );
        assert_eq!(
            store.read("ok.txt").unwrap(),
            None,
            "the valid half of {bad:?} was written"
        );
        assert!(matches!(
            store.read(bad),
            Err(StoreError::InvalidPath { .. })
        ));
    }
    // a file and a directory of one name in one edit
    let err = store
        .apply(
            &Edit::new().put("p", "1").put("p/q", "2"),
            "clash",
            &Reapply,
        )
        .unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
}

#[test]
fn directory_backend() {
    let d = tempfile::tempdir().unwrap();
    suite(&DirStore::new(d.path().join("data")), Freshness::Local);
}

#[test]
fn ref_backend_in_ref_form() {
    let w = World::new();
    let store = RefStore::new(
        w.clone_dir("a"),
        w.remote(),
        RefPair::ref_form("registry/probe").unwrap(),
    )
    .with_retry(RetryPolicy::immediate(3));
    suite(&store, Freshness::Empty);
    assert!(w.tip("refs/mock/registry/probe").is_some());
}

#[test]
fn ref_backend_in_branch_form() {
    let w = World::new();
    let store = RefStore::new(
        w.clone_dir("a"),
        w.remote(),
        RefPair::branch_form("registry/probe").unwrap(),
    )
    .with_retry(RetryPolicy::immediate(3));
    suite(&store, Freshness::Empty);
    assert!(w.tip("refs/heads/mock/registry/probe").is_some());
    assert!(
        w.tip("refs/mock/registry/probe").is_none(),
        "the branch form leaves nothing outside refs/heads on the remote"
    );
}

#[test]
fn a_temporary_file_of_the_directory_backend_is_never_listed() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("data");
    let s = DirStore::new(&root);
    s.apply(&Edit::new().put("a/b", "1"), "m", &Reapply)
        .unwrap();
    std::fs::write(root.join("a/.mockspace-store-tmp-1-1"), "left by a crash").unwrap();
    assert_eq!(s.list("").unwrap(), ["a/b"]);
}
