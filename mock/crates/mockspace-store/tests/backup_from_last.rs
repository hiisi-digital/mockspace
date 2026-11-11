//! The copy of an instance is built from the last copy, with only what changed handed to git, so
//! it must come out as a copy built whole would: a file where a directory was, a directory where a
//! file was, and a file gone from the instance; and a first guard, built from the base, must hold
//! every file of the instance.

mod common;

use common::{World, git, inst_dir, seeded, write};
use mockspace_store::Instance;

/// Every path of the guard set of the backup ref, from the instance, and the bytes of each.
fn guarded(repo: &std::path::Path, i: &Instance<'_>) -> Vec<(String, String)> {
    let reference = i.backup_ref();
    let mut out = Vec::new();
    for path in git(repo, &["ls-tree", "-r", "--name-only", &reference]).lines() {
        let Some((_, rel)) = path.split_once("/guard/") else { continue };
        out.push((rel.to_string(), git(repo, &["show", &format!("{reference}:{path}")])));
    }
    out.sort();
    out
}

// Mutation: seed the copy's index from the last copy and add what changed without taking out what
// is gone, or add a file where the last copy had a directory without replacing it; the guard then
// holds a file the instance no longer has, or the copy fails.
#[test]
fn a_guard_built_from_the_last_holds_what_the_instance_holds_now() {
    let w = World::new();
    let a = seeded(&w);
    let repo = w.dir.path().join("a");
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "shape/inner.md", "a file in a directory\n");
    write(&dir, "flat.md", "a file that becomes a directory\n");
    write(&dir, "gone.md", "a file taken out\n");
    i.guard().unwrap().copied.unwrap();
    let first = guarded(&repo, &i);
    assert!(first.iter().any(|(p, _)| p == "shape/inner.md"), "the control: the first guard holds the file: {first:?}");
    // the first guard is the base and what changed against it, every file of the instance
    let mut on_disk: Vec<String> = ["flat.md", "gone.md", "shape/inner.md", "ticket/t1.toml", "ticket/t2.toml"].map(String::from).to_vec();
    on_disk.sort();
    assert_eq!(first.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), on_disk);
    std::fs::remove_dir_all(dir.join("shape")).unwrap();
    write(&dir, "shape", "a file where the directory was\n");
    std::fs::remove_file(dir.join("flat.md")).unwrap();
    write(&dir, "flat.md/now.md", "a directory where the file was\n");
    std::fs::remove_file(dir.join("gone.md")).unwrap();
    i.guard().unwrap().copied.unwrap();
    let now = guarded(&repo, &i);
    let names: Vec<&str> = now.iter().map(|(p, _)| p.as_str()).collect();
    assert!(names.contains(&"shape") && !names.contains(&"shape/inner.md"), "{now:?}");
    assert!(names.contains(&"flat.md/now.md") && !names.contains(&"flat.md"), "{now:?}");
    assert!(!names.contains(&"gone.md"), "{now:?}");
    let shape = now.iter().find(|(p, _)| p == "shape").unwrap();
    assert_eq!(shape.1, "a file where the directory was");
}
