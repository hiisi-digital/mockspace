//! The edges of an instance that holds files by pattern: what it must refuse before it writes.
//!
//! Each case here was first a way to lose a file: a ref emptied by narrowed patterns, a branch
//! file hidden by an exclude line, a write that went through a symlink, an update that stopped
//! half done, an instance whose files a clean removed.
mod common;

use std::path::{Path, PathBuf};

use common::{World, git, store};
use mockspace_store::{Held, Instance, Refuse, Store, StoreError};

fn clone_path(w: &World, name: &str) -> PathBuf {
    w.dir.path().join(name)
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn held(patterns: &[&str]) -> Held {
    Held::new(patterns).unwrap()
}

const TEMPLATES: &[&str] = &["**/*.md.tmpl"];

/// Clone `a` with a crates directory of two sources and two templates, the templates seeded onto
/// the ref.
fn seeded(w: &World) -> (mockspace_store::RefStore, PathBuf) {
    let a = store(w, "a");
    let dir = clone_path(w, "a").join("crates");
    write(&dir, "one/src/lib.rs", "pub fn one() {}\n");
    write(&dir, "one/DESIGN.md.tmpl", "design of one\n");
    write(&dir, "two/src/lib.rs", "pub fn two() {}\n");
    write(&dir, "two/README.md.tmpl", "readme of two\n");
    Instance::new(&a, &dir).unwrap().holding(held(TEMPLATES)).seed("seed", &Refuse).unwrap();
    (a, dir)
}

fn why(e: StoreError) -> String {
    e.to_string()
}

#[test]
fn narrowed_patterns_do_not_read_the_rest_of_the_ref_as_removed() {
    let w = World::new();
    let (a, dir) = seeded(&w);
    let tip = a.local_tip().unwrap();
    let narrow = Instance::new(&a, &dir).unwrap().holding(held(&["one/*.md.tmpl"]));
    let refused = why(narrow.edit().unwrap_err());
    assert!(refused.contains("outside its patterns") && refused.contains("two/README.md.tmpl"), "{refused}");
    assert!(narrow.apply("narrowed", &Refuse).is_err());
    assert_eq!(a.local_tip().unwrap(), tip, "the ref moved under patterns that do not cover it");
    // the control: the patterns it was made with read it as clean
    assert!(!Instance::new(&a, &dir).unwrap().holding(held(TEMPLATES)).is_dirty().unwrap());
}

/// What `git status` lists as untracked under the crates directory of `clone`.
fn untracked(clone: &Path) -> Vec<String> {
    git(clone, &["status", "--porcelain", "-uall", "--", "crates"])
        .lines()
        .filter_map(|l| l.strip_prefix("?? ").map(str::to_owned))
        .collect()
}

#[test]
fn the_exclude_lines_hide_the_files_held_and_no_file_of_the_branch() {
    for (patterns, hidden, shown) in [
        (&["*"][..], &["top.md"][..], &["crates/notes/sub/branch.rs", "crates/notes/a.md"][..]),
        (&["*/**"][..], &["closed/topic.md", "closed/deep/er.md"][..], &["crates/open.md"][..]),
        (&["**/*.md.tmpl"][..], &["x/y/D.md.tmpl", "D.md.tmpl"][..], &["crates/x/y/lib.rs", "crates/x.rs"][..]),
        (&["notes/*.md"][..], &["notes/a.md"][..], &["crates/notes/sub/b.md", "crates/other/a.md"][..]),
    ] {
        let w = World::new();
        let a = store(&w, "a");
        let clone = clone_path(&w, "a");
        let dir = clone.join("crates");
        for rel in hidden {
            write(&dir, rel, "held\n");
        }
        for rel in shown {
            write(&clone, rel, "the branch's\n");
        }
        let inst = Instance::new(&a, &dir).unwrap().holding(held(patterns));
        inst.seed("seed", &Refuse).unwrap();
        let mut seen = untracked(&clone);
        seen.sort();
        let mut want: Vec<String> = shown.iter().map(|s| (*s).to_owned()).collect();
        want.sort();
        assert_eq!(seen, want, "{patterns:?}: git sees other files than the ones the instance leaves alone");
    }
}

#[cfg(unix)]
#[test]
fn nothing_is_written_or_removed_through_a_symlinked_directory() {
    let w = World::new();
    let _ = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let outside = w.dir.path().join("outside");
    write(&outside, "DESIGN.md.tmpl", "somebody else's file\n");
    let dir_b = clone_path(&w, "b").join("crates");
    std::fs::create_dir_all(&dir_b).unwrap();
    std::os::unix::fs::symlink(&outside, dir_b.join("one")).unwrap();
    let inst = Instance::new(&b, &dir_b).unwrap().holding(held(TEMPLATES));
    let refused = why(inst.materialize().unwrap_err());
    assert!(refused.contains("symlink") && refused.contains("one"), "{refused}");
    assert_eq!(std::fs::read_to_string(outside.join("DESIGN.md.tmpl")).unwrap(), "somebody else's file\n");
    assert!(!dir_b.join("two/README.md.tmpl").exists(), "a refused instance wrote a file");
    // the control: with the link gone the same call makes the instance
    std::fs::remove_file(dir_b.join("one")).unwrap();
    inst.materialize().unwrap();
    assert_eq!(std::fs::read_to_string(dir_b.join("one/DESIGN.md.tmpl")).unwrap(), "design of one\n");
}

#[test]
fn an_update_a_branch_file_stands_in_the_way_of_changes_nothing() {
    let w = World::new();
    let (a, dir) = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("crates");
    write(&dir_b, "zzz", "a branch file\n");
    let ib = Instance::new(&b, &dir_b).unwrap().holding(held(TEMPLATES));
    let base = ib.materialize().unwrap();

    let ia = Instance::new(&a, &dir).unwrap().holding(held(TEMPLATES));
    write(&dir, "aaa/NEW.md.tmpl", "new\n");
    write(&dir, "zzz/X.md.tmpl", "under what is a file in the other clone\n");
    std::fs::remove_file(dir.join("two/README.md.tmpl")).unwrap();
    ia.apply("edit", &Refuse).unwrap();

    let refused = why(ib.update().unwrap_err());
    assert!(refused.contains("in the way") && refused.contains("zzz/X.md.tmpl"), "{refused}");
    assert!(!dir_b.join("aaa/NEW.md.tmpl").exists(), "a refused update wrote part of the tip");
    assert!(dir_b.join("two/README.md.tmpl").exists(), "a refused update removed a file");
    assert_eq!(ib.base().unwrap().as_deref(), Some(base.as_str()));
    assert_eq!(std::fs::read_to_string(dir_b.join("zzz")).unwrap(), "a branch file\n");
    // the control: with the branch file out of the way the same update lands whole
    std::fs::remove_file(dir_b.join("zzz")).unwrap();
    ib.update().unwrap();
    assert!(dir_b.join("aaa/NEW.md.tmpl").exists() && dir_b.join("zzz/X.md.tmpl").exists());
    assert!(!dir_b.join("two/README.md.tmpl").exists());
}

#[test]
fn a_tip_with_a_file_outside_the_patterns_is_not_followed() {
    let w = World::new();
    let (a, dir) = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("crates");
    let ib = Instance::new(&b, &dir_b).unwrap().holding(held(TEMPLATES));
    ib.materialize().unwrap();
    // a writer with wider patterns puts a source onto the ref
    write(&dir, "one/extra.rs", "pub fn extra() {}\n");
    let wide = Instance::new(&a, &dir).unwrap().holding(held(&["**/*.md.tmpl", "one/extra.rs"]));
    wide.apply("wider", &Refuse).unwrap();
    let refused = why(ib.update().unwrap_err());
    assert!(refused.contains("outside its patterns") && refused.contains("one/extra.rs"), "{refused}");
    assert!(!dir_b.join("one/extra.rs").exists());
}

#[test]
fn closed_rounds_are_held_and_the_open_round_stays_with_the_branch() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = clone_path(&w, "a").join("design_rounds");
    write(&dir, "202611010000/202611010000_topic.closed.md", "closed\n");
    write(&dir, "202611010000/deep/er.md", "deeper\n");
    write(&dir, "202611110000_topic.open.md", "open\n");
    let ia = Instance::new(&a, &dir).unwrap().holding(held(&["*/**"]));
    ia.seed("seed", &Refuse).unwrap();
    let on_ref = git(&clone_path(&w, "a"), &["ls-tree", "-r", "--name-only", &a.local_tip().unwrap().unwrap()]);
    assert_eq!(on_ref.lines().collect::<Vec<_>>(), ["202611010000/202611010000_topic.closed.md", "202611010000/deep/er.md"]);

    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("design_rounds");
    write(&dir_b, "202611110000_topic.open.md", "open, as the branch has it\n");
    let ib = Instance::new(&b, &dir_b).unwrap().holding(held(&["*/**"]));
    ib.materialize().unwrap();
    assert_eq!(std::fs::read_to_string(dir_b.join("202611010000/deep/er.md")).unwrap(), "deeper\n");
    assert!(!ib.is_dirty().unwrap(), "the open round's file reads as an edit of the ref");
}

#[test]
fn files_a_clean_removed_are_written_again_from_the_base() {
    let w = World::new();
    let (a, dir) = seeded(&w);
    let ia = Instance::new(&a, &dir).unwrap().holding(held(TEMPLATES));
    assert_eq!(ia.present().unwrap(), (2, 2));
    std::fs::remove_file(dir.join("one/DESIGN.md.tmpl")).unwrap();
    std::fs::remove_file(dir.join("two/README.md.tmpl")).unwrap();
    assert_eq!(ia.present().unwrap(), (0, 2));
    assert_eq!(ia.restore().unwrap(), 2);
    assert_eq!(std::fs::read_to_string(dir.join("two/README.md.tmpl")).unwrap(), "readme of two\n");
    assert!(!ia.is_dirty().unwrap());
    // a file that is here is left as it is, changed or not
    write(&dir, "one/DESIGN.md.tmpl", "changed here\n");
    assert_eq!(ia.restore().unwrap(), 0);
    assert_eq!(std::fs::read_to_string(dir.join("one/DESIGN.md.tmpl")).unwrap(), "changed here\n");
}

#[cfg(unix)]
#[test]
fn an_update_does_not_write_through_a_file_that_is_a_symlink() {
    let w = World::new();
    let (a, dir) = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("crates");
    let ib = Instance::new(&b, &dir_b).unwrap().holding(held(TEMPLATES));
    ib.materialize().unwrap();
    // the file becomes a link to one outside that holds the same text, so it reads as unchanged
    let outside = w.dir.path().join("outside.txt");
    std::fs::write(&outside, "design of one\n").unwrap();
    std::fs::remove_file(dir_b.join("one/DESIGN.md.tmpl")).unwrap();
    std::os::unix::fs::symlink(&outside, dir_b.join("one/DESIGN.md.tmpl")).unwrap();

    write(&dir, "one/DESIGN.md.tmpl", "second\n");
    Instance::new(&a, &dir).unwrap().holding(held(TEMPLATES)).apply("edit", &Refuse).unwrap();

    let refused = why(ib.update().unwrap_err());
    assert!(refused.contains("symlink") && refused.contains("one/DESIGN.md.tmpl"), "{refused}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "design of one\n", "the update wrote through the link");
    // the control: a regular file there again, the same update lands
    std::fs::remove_file(dir_b.join("one/DESIGN.md.tmpl")).unwrap();
    write(&dir_b, "one/DESIGN.md.tmpl", "design of one\n");
    ib.update().unwrap();
    assert_eq!(std::fs::read_to_string(dir_b.join("one/DESIGN.md.tmpl")).unwrap(), "second\n");
}
