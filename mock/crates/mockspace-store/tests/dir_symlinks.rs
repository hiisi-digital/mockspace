//! What the directory backend does with a symlink, which is what the walk and the `fs::write` of the
//! registry did before it went through a store (the test history replaced a link with a rename, and
//! now writes through it): a symlinked directory is walked into, a symlinked file is written through, and neither lets a cycle or a dangling link
//! hang or fail a listing.

#![cfg(unix)]

use std::os::unix::fs::symlink;

use mockspace_store::{DirStore, Edit, Reader, Reapply, Store};

#[test]
fn a_symlinked_directory_is_walked_into_as_the_old_walk_did() {
    let d = tempfile::tempdir().unwrap();
    let (root, other) = (d.path().join("root"), d.path().join("other"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(other.join("deep")).unwrap();
    std::fs::write(other.join("a.toml"), "a = 1").unwrap();
    std::fs::write(other.join("deep/b.toml"), "b = 2").unwrap();
    std::fs::write(root.join("own.toml"), "o = 0").unwrap();
    symlink(&other, root.join("linked")).unwrap();
    let s = DirStore::new(&root);
    assert_eq!(s.list("").unwrap(), [
        "linked/a.toml",
        "linked/deep/b.toml",
        "own.toml"
    ]);
    assert_eq!(
        s.read("linked/deep/b.toml").unwrap().as_deref(),
        Some(&b"b = 2"[..])
    );
}

#[test]
fn a_write_to_a_symlinked_file_goes_to_its_target_and_the_link_stays() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let target = d.path().join("elsewhere.toml");
    std::fs::write(&target, "old").unwrap();
    symlink(&target, root.join("f.toml")).unwrap();
    let s = DirStore::new(&root);
    let r = s
        .apply(&Edit::new().put("f.toml", "new"), "m", &Reapply)
        .unwrap();
    assert!(r.changed);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    assert!(
        std::fs::symlink_metadata(root.join("f.toml"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link was replaced by a file of its own"
    );
    assert_eq!(s.read("f.toml").unwrap().as_deref(), Some(&b"new"[..]));
}

#[test]
fn a_write_to_a_dangling_symlink_creates_its_target_as_fs_write_did() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let target = d.path().join("not-yet.toml");
    symlink(&target, root.join("f.toml")).unwrap();
    let s = DirStore::new(&root);
    s.apply(&Edit::new().put("f.toml", "made"), "m", &Reapply)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "made");
    assert!(
        std::fs::symlink_metadata(root.join("f.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_symlink_back_to_an_ancestor_does_not_loop_the_listing() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/a.toml"), "a").unwrap();
    symlink(&root, root.join("sub/up")).unwrap();
    let s = DirStore::new(&root);
    assert_eq!(s.list("").unwrap(), ["sub/a.toml"]);
}

#[test]
fn a_dangling_symlink_is_listed_by_name_and_reads_as_absent() {
    // the old walk took anything that was not a directory for a file and left the read to fail
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    symlink(d.path().join("gone"), root.join("dangling.toml")).unwrap();
    let s = DirStore::new(&root);
    assert_eq!(s.list("").unwrap(), ["dangling.toml"]);
    assert_eq!(s.read("dangling.toml").unwrap(), None);
}

#[test]
fn a_write_through_a_symlink_with_a_relative_target_lands_beside_the_link() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("target-of-the-relative-link.toml"), "old").unwrap();
    // `sub/f.toml` points at `../target-of-the-relative-link.toml`, resolved from the link's directory
    symlink(
        "../target-of-the-relative-link.toml",
        root.join("sub/f.toml"),
    )
    .unwrap();
    let s = DirStore::new(&root);
    s.apply(&Edit::new().put("sub/f.toml", "new"), "m", &Reapply)
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("target-of-the-relative-link.toml")).unwrap(),
        "new"
    );
    // resolved from the working directory instead, the write would land outside the tempdir
    assert!(!std::path::Path::new("../target-of-the-relative-link.toml").exists());
    assert!(
        std::fs::symlink_metadata(root.join("sub/f.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_write_through_a_cycle_of_links_fails_and_leaves_the_link() {
    // `fs::write` followed links until the system gave up and then failed (ELOOP)
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    symlink("b.toml", root.join("a.toml")).unwrap();
    symlink("a.toml", root.join("b.toml")).unwrap();
    let s = DirStore::new(&root);
    let err = s
        .apply(&Edit::new().put("a.toml", "x"), "m", &Reapply)
        .unwrap_err();
    assert!(
        matches!(err, mockspace_store::StoreError::Io { .. }),
        "{err}"
    );
    assert!(
        std::fs::symlink_metadata(root.join("a.toml"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link was replaced by a file"
    );
}

#[test]
fn a_write_to_a_link_whose_target_directory_is_missing_fails_and_creates_nothing() {
    // `fs::write` did not create directories, so a link into a directory that is not there fails
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let outside = d.path().join("outside");
    symlink(outside.join("deeper/f.toml"), root.join("f.toml")).unwrap();
    let s = DirStore::new(&root);
    let err = s
        .apply(&Edit::new().put("f.toml", "x"), "m", &Reapply)
        .unwrap_err();
    assert!(
        matches!(err, mockspace_store::StoreError::Io { .. }),
        "{err}"
    );
    assert!(
        !outside.exists(),
        "a directory was created outside the store"
    );
}
