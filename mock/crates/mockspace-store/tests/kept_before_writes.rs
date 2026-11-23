//! Every place the store writes over, changes or removes a file it did not make in the same command
//! keeps a copy of it in the cache first (`mockspace_store::kept`). Each test makes one of those
//! writes and asks the copies this process kept for the bytes that stood there.

#[path = "../../../../tests/common/ambient.rs"]
mod ambient;
mod common;

use std::path::Path;

use common::{T1, T2, World, inst_dir, read, seeded, store, write};
use mockspace_store::kept::copies_of;
use mockspace_store::{DirStore, Edit, Instance, Reapply, Store};

/// The bytes of every file copy this process kept of `path`.
fn kept(path: &Path) -> Vec<Vec<u8>> {
    copies_of(path).unwrap().iter().filter(|p| p.is_file()).map(|p| std::fs::read(p).unwrap()).collect()
}

// Mutation: drop the copy in the instance's `write` (`instance/files.rs`) or before a removal of
// `follow` (`instance/apply.rs`); an update then writes the tip over the file, or removes it, and
// the bytes that stood there are nowhere.
#[test]
fn an_update_keeps_a_copy_of_each_file_it_writes_over_or_removes() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "data");
    let inst = Instance::new(&b, &dir).unwrap();
    inst.materialize().unwrap();
    a.apply(&Edit::new().put("ticket/t2.toml", "id = \"t2\"\ntheirs = 1\n").remove("ticket/t1.toml"), "tip", &Reapply).unwrap();
    inst.update().unwrap();
    assert_eq!(read(&dir, "ticket/t2.toml"), "id = \"t2\"\ntheirs = 1\n", "the control: the update wrote the tip");
    assert!(!dir.join("ticket/t1.toml").exists(), "the control: the update removed the file");
    assert!(kept(&dir.join("ticket/t2.toml")).contains(&T2.as_bytes().to_vec()), "the file written over is kept");
    assert!(kept(&dir.join("ticket/t1.toml")).contains(&T1.as_bytes().to_vec()), "the file removed is kept");
}

// Mutation: drop the copy before `follow` takes a directory out of the way of a file of the tip
// (`instance/apply.rs`); what the directory held is then nowhere.
#[test]
fn an_update_keeps_a_copy_of_a_directory_it_takes_out_of_the_way_of_a_file() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "data");
    let inst = Instance::new(&b, &dir).unwrap();
    inst.materialize().unwrap();
    std::fs::create_dir_all(dir.join("ticket/t3.toml/below")).unwrap();
    a.apply(&Edit::new().put("ticket/t3.toml", "id = \"t3\"\n"), "tip", &Reapply).unwrap();
    inst.update().unwrap();
    assert_eq!(read(&dir, "ticket/t3.toml"), "id = \"t3\"\n", "the control: the file is written where the directory was");
    let copies = copies_of(&dir.join("ticket/t3.toml")).unwrap();
    assert!(copies.iter().any(|c| c.join("below").is_dir()), "the directory is kept: {copies:?}");
}

// Mutation: drop the copy before `restore_written_over` takes a directory out of the way
// (`instance/backup.rs`); what stood at the path is then nowhere.
#[test]
fn a_restore_keeps_a_copy_of_a_directory_standing_where_the_file_was() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "data");
    let inst = Instance::new(&a, &dir).unwrap();
    inst.materialize().unwrap();
    write(&dir, "ticket/t1.toml", "id = \"t1\"\nedited = 1\n");
    inst.guard().unwrap();
    std::fs::remove_file(dir.join("ticket/t1.toml")).unwrap();
    std::fs::create_dir_all(dir.join("ticket/t1.toml/below")).unwrap();
    inst.restore_written_over(&["ticket/t1.toml".to_string()]).unwrap();
    assert_eq!(read(&dir, "ticket/t1.toml"), "id = \"t1\"\nedited = 1\n", "the control: the edit is back");
    let copies = copies_of(&dir.join("ticket/t1.toml")).unwrap();
    assert!(copies.iter().any(|c| c.join("below").is_dir()), "the directory is kept: {copies:?}");
}

// Mutation: drop the copy before the exclude file is appended to (`instance/records.rs`); the lines
// the person kept there are then in no copy if the append goes wrong.
#[test]
fn an_instance_keeps_a_copy_of_the_exclude_file_before_it_changes_it() {
    let w = World::new();
    let a = seeded(&w);
    let exclude = w.dir.path().join("a").join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "# the person's own\n/mine/\n").unwrap();
    let dir = inst_dir(&w, "a", "data");
    Instance::new(&a, &dir).unwrap().materialize().unwrap();
    assert_ne!(std::fs::read_to_string(&exclude).unwrap(), "# the person's own\n/mine/\n", "the control: the line was appended");
    assert!(kept(&exclude).contains(&b"# the person's own\n/mine/\n".to_vec()), "the exclude file is kept as it was");
}

// Mutation: drop either copy of the directory store's `apply` (`dir.rs`); a write over a file, or
// its removal, then leaves the bytes from before nowhere.
#[test]
fn a_directory_store_keeps_a_copy_of_a_file_before_writing_over_or_removing_it() {
    let d = mockspace_manifest::scratch::tempdir().unwrap();
    let root = d.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("over.toml"), "over = 0").unwrap();
    std::fs::write(root.join("gone.toml"), "gone = 0").unwrap();
    let s = DirStore::new(&root);
    s.apply(&Edit::new().put("over.toml", "over = 1").remove("gone.toml"), "change", &Reapply).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("over.toml")).unwrap(), "over = 1", "the control: written over");
    assert!(!root.join("gone.toml").exists(), "the control: removed");
    assert!(kept(&root.join("over.toml")).contains(&b"over = 0".to_vec()), "the file written over is kept");
    assert!(kept(&root.join("gone.toml")).contains(&b"gone = 0".to_vec()), "the file removed is kept");
}
