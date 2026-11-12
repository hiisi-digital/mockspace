//! What a commit puts on the ref: each file's bytes exactly as given and its mode, however many
//! files it holds, whatever the attributes and filters of the repository it is made in. The blobs
//! of a commit are written by one git process, from files staged in the git directory, so git
//! would apply the filters the attributes name for those files unless it is told not to. A commit
//! or a look of thousands of files returns: git is fed its input while what it prints is read.
mod common;

use common::{World, git, inst_dir, seeded, store, write};
use mockspace_store::{Edit, Instance, Reapply, Store};

// Mutation: hash the staged files without `--no-filters`; the filter and the line endings then
// change the bytes on the ref.
#[test]
fn a_commit_of_many_files_holds_each_ones_bytes_and_mode_through_any_filter() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = w.dir.path().join("a");
    // every path, the staged ones in the git directory too, goes through a filter and crlf
    std::fs::write(dir.join(".git/info/attributes"), "* text eol=crlf ident filter=upper\n").unwrap();
    git(&dir, &["config", "filter.upper.clean", "tr a-z A-Z"]);
    git(&dir, &["config", "filter.upper.required", "true"]);
    git(&dir, &["config", "core.autocrlf", "true"]);
    let files: Vec<(String, Vec<u8>)> = (0..450)
        .map(|n| {
            let bytes = match n % 3 {
                0 => format!("line {n}\nand $Id$\n").into_bytes(),
                1 => format!("crlf {n}\r\nmixed\n").into_bytes(),
                _ => vec![0, 159, 146, 150, b'\r', b'\n', n as u8],
            };
            (format!("d{}/f{n}", n % 7), bytes)
        })
        .collect();
    let mut edit = Edit::new().put_exec("run.sh", b"#!/bin/sh\necho lower\n".to_vec());
    for (path, bytes) in &files {
        edit = edit.put(path.clone(), bytes.clone());
    }
    a.apply(&edit, "many", &Reapply).unwrap();
    let tip = git(&dir, &["rev-parse", common::MOCK_REF]);
    let held = a.files_of(&tip).unwrap();
    assert_eq!(held.len(), files.len() + 1, "every file is on the ref");
    for (path, bytes) in &files {
        assert_eq!(held.get(path), Some(bytes), "{path} holds its bytes exactly");
    }
    assert_eq!(held.get("run.sh").map(Vec::as_slice), Some(&b"#!/bin/sh\necho lower\n"[..]));
    let modes = git(&dir, &["ls-tree", "-r", &tip]);
    assert!(modes.lines().any(|l| l.starts_with("100755 ") && l.ends_with("\trun.sh")), "{modes}");
    assert_eq!(modes.lines().filter(|l| l.starts_with("100644 ")).count(), files.len(), "{modes}");
    let staged: Vec<_> = std::fs::read_dir(dir.join(".git"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("mockspace-store-"))
        .collect();
    assert!(staged.is_empty(), "nothing staged for the commit is left: {staged:?}");
}

// Mutation: write the whole input to git before reading what it prints; once git has printed more
// than a pipe holds it waits for a reader, the writer waits for git, and the commit never returns.
#[test]
fn a_commit_of_thousands_of_files_returns() {
    let w = World::new();
    let a = store(&w, "a");
    let mut edit = Edit::new();
    for n in 0..6000 {
        edit = edit.put(format!("d{}/file-{n}", n % 50), format!("{n}\n"));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let done = a.apply(&edit, "thousands", &Reapply).map(|_| ());
        let _ = tx.send(done.map_err(|e| e.to_string()));
    });
    let done = rx.recv_timeout(std::time::Duration::from_secs(120)).expect("the commit returns within two minutes");
    done.unwrap();
    let dir = w.dir.path().join("a");
    let held = git(&dir, &["ls-tree", "-r", "--name-only", common::MOCK_REF]);
    assert_eq!(held.lines().count(), 6000);
}

// Mutation: the same, for the copy a look makes of thousands of new files in an instance, which is
// what git's own hook runs while the person's checkout waits on it.
#[test]
fn a_look_at_thousands_of_new_files_in_an_instance_returns() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let i = Instance::new(&a, &dir).unwrap();
        i.materialize().unwrap();
        for n in 0..6000 {
            write(&dir, &format!("bulk/d{}/file-{n}.md", n % 50), &format!("{n}\n"));
        }
        let _ = tx.send(i.look().map(|_| ()).map_err(|e| e.to_string()));
    });
    let done = rx.recv_timeout(std::time::Duration::from_secs(120)).expect("the look returns within two minutes");
    done.unwrap();
}
