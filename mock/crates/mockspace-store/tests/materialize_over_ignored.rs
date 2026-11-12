//! `materialize` over a directory that holds files git ignores: build output, a secret, a file
//! where the ref has a directory. Those files are the person's and were never the ref's, so the
//! fill neither writes over them nor, when it fails, removes them: a failed fill takes back only
//! what it wrote.
mod common;

use common::{World, inst_dir, seeded, write};
use mockspace_store::{Edit, Instance, Reapply, Store};

fn clone_path(w: &World, name: &str) -> std::path::PathBuf {
    w.dir.path().join(name)
}

/// The person's own files in the instance's directory, each one git ignores, with the
/// `.gitignore` that says so: `ticket` is a file where the ref holds `ticket/t1.toml`.
fn ignored_files(w: &World, dir: &std::path::Path) {
    std::fs::write(clone_path(w, "a").join(".gitignore"), "*.env\nticket\ntarget/\n").unwrap();
    write(dir, "keep.env", "A SECRET\n");
    write(dir, "target/out.o", "build output\n");
}

fn assert_kept(dir: &std::path::Path) {
    assert_eq!(std::fs::read_to_string(dir.join("keep.env")).unwrap(), "A SECRET\n");
    assert_eq!(std::fs::read_to_string(dir.join("target/out.o")).unwrap(), "build output\n");
}

// Control: the ref holds `secret.env` and the clone keeps its own; the fill does not write over
// it, whatever the ignore rules say.
//
// Mutation: let the walk pass over an ignored file the tip holds; the fill then writes the ref's
// bytes over the person's.
#[test]
fn materialize_writes_over_no_ignored_file_the_tip_holds() {
    let w = World::new();
    let a = seeded(&w);
    a.apply(&Edit::new().put("secret.env", "THE REF'S\n"), "add", &Reapply).unwrap();
    std::fs::write(clone_path(&w, "a").join(".gitignore"), "*.env\n").unwrap();
    let dir = inst_dir(&w, "a", "registry");
    write(&dir, "secret.env", "MINE, NEVER COMMITTED\n");
    let r = Instance::new(&a, &dir).unwrap().materialize();
    assert!(r.is_err(), "the fill would write over the person's file: {r:?}");
    assert_eq!(std::fs::read_to_string(dir.join("secret.env")).unwrap(), "MINE, NEVER COMMITTED\n");
}

// Mutation: let the fill start with an ignored file standing where the tip has a directory; it
// fails at `ticket/t1.toml` and the clean-up of a whole-directory instance empties the directory,
// the secret and the build output with it.
#[test]
fn a_file_where_the_tip_has_a_directory_is_refused_and_no_ignored_file_is_removed() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    ignored_files(&w, &dir);
    write(&dir, "ticket", "my own notes\n");
    let r = Instance::new(&a, &dir).unwrap().materialize();
    let e = r.expect_err("the tip cannot be written where a file stands").to_string();
    assert!(e.contains("ticket"), "the refusal names the file in the way: {e}");
    assert_kept(&dir);
    assert_eq!(std::fs::read_to_string(dir.join("ticket")).unwrap(), "my own notes\n");
}

// Mutation: on a failed fill, empty the directory of a whole-directory instance, or remove every
// path of the tip whoever wrote it; the person's ignored files then go with the fill's.
#[test]
fn a_fill_that_fails_after_writing_takes_back_only_what_it_wrote() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    ignored_files(&w, &dir);
    // the step after the fill writes the exclude file, so a directory there fails it
    let exclude = clone_path(&w, "a").join(".git/info/exclude");
    let _ = std::fs::remove_file(&exclude);
    std::fs::create_dir_all(exclude.join("in-the-way")).unwrap();
    let r = Instance::new(&a, &dir).unwrap().materialize();
    assert!(r.is_err(), "the control: the step after the fill fails: {r:?}");
    assert_kept(&dir);
    assert!(!dir.join("ticket").exists(), "what the fill wrote is taken back");
    assert!(Instance::new(&a, &dir).unwrap().base().unwrap().is_none(), "no record is left");
}
