//! Names that are not text against the ignore rules. Git matches a `.gitignore` line against the
//! bytes of a name, so two names that read as the same lossy text get each its own verdict: the
//! one ignored is the person's and left alone, the other is refused by name, since a ref names its
//! files as text and cannot hold it.
#![cfg(unix)]
mod common;

use std::os::unix::ffi::OsStrExt;

use common::{World, inst_dir, seeded};
use mockspace_store::Instance;

// Mutation: match the ignore listing by lossy name, as before; the name git does not ignore then
// shares the verdict of the one it does, and is passed over without a word.
#[test]
fn two_names_with_one_lossy_reading_get_each_its_own_ignore_verdict() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let mut rule = b"a".to_vec();
    rule.extend_from_slice(b"\xff.md\n");
    std::fs::write(w.dir.path().join("a").join(".gitignore"), &rule).unwrap();
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"notes/a\xff.md")), "ignored\n").unwrap();
    std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"notes/a\xfe.md")), "not ignored\n").unwrap();
    let err = i.edit().unwrap_err().to_string();
    assert!(err.contains("not text") && err.contains("notes/a"), "the one git does not ignore is refused by name: {err}");
    std::fs::remove_file(dir.join(std::ffi::OsStr::from_bytes(b"notes/a\xfe.md"))).unwrap();
    let ignored = i.ignored().unwrap();
    assert_eq!(ignored.len(), 1, "the one git ignores is named once, and left: {ignored:?}");
    assert!(i.edit().unwrap().is_empty(), "nothing of it goes into an edit");
}
