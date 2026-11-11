//! An instance that holds only the files its patterns name, in a directory the branch shares.
//!
//! The directory is one the branch has files in too (a crate's sources beside its templates,
//! the open round's flat files beside the closed rounds). The instance reads, writes and
//! excludes only what its patterns match, and leaves every other file alone.
#![allow(unused_imports)]
mod common;

use std::path::{Path, PathBuf};

use common::{World, git, store};
use mockspace_store::{Edit, Held, Instance, Reapply, Refuse, Store, StoreError};

fn clone_path(w: &World, name: &str) -> PathBuf {
    w.dir.path().join(name)
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn templates() -> Held {
    Held::new(&["**/*.md.tmpl"]).unwrap()
}

/// A crates directory in clone `name` with two sources and two templates.
fn crates(w: &World, name: &str) -> PathBuf {
    let dir = clone_path(w, name).join("crates");
    write(&dir, "one/src/lib.rs", "pub fn one() {}\n");
    write(&dir, "one/DESIGN.md.tmpl", "design of one\n");
    write(&dir, "two/src/lib.rs", "pub fn two() {}\n");
    write(&dir, "two/README.md.tmpl", "readme of two\n");
    dir
}

#[test]
fn a_pattern_matches_whole_segments_and_a_double_star_any_depth() {
    let held = templates();
    assert!(held.holds("DESIGN.md.tmpl"));
    assert!(held.holds("one/DESIGN.md.tmpl"));
    assert!(held.holds("a/b/c/README.md.tmpl"));
    assert!(!held.holds("one/src/lib.rs"));
    assert!(!held.holds("one/DESIGN.md"), "the control: the rendered file is not a template");
    assert!(!held.holds("one/DESIGN.md.tmpl/x"), "a pattern names a file, not what is under it");

    let closed = Held::new(&["*/**"]).unwrap();
    assert!(closed.holds("202611010000/topic.md"));
    assert!(closed.holds("202611010000/deep/er/file"));
    assert!(!closed.holds("202611010000_topic.open.md"), "the control: a flat file is not held");

    let one = Held::new(&["notes/*.md", "exact.toml"]).unwrap();
    assert!(one.holds("notes/a.md") && one.holds("exact.toml"));
    assert!(!one.holds("notes/deep/a.md"), "a single star stays inside its segment");
    assert!(!one.holds("other/exact.toml"));
}

#[test]
fn a_pattern_that_cannot_be_read_as_one_is_refused() {
    for bad in ["", "/abs", "a/../b", "a//b", "a/", "what?", "set[ab]", "a/***/b", "!not", "sp ace"] {
        assert!(Held::new(&[bad]).is_err(), "`{bad}` was taken");
    }
    assert!(Held::new::<&str>(&[]).is_err(), "no pattern holds nothing, which is no instance");
    // the control: the forms the tests above use are taken
    assert!(Held::new(&["**/*.md.tmpl", "*/**", "a/b.toml", "**"]).is_ok());
}

#[test]
fn seeding_puts_only_the_held_files_on_the_ref_and_leaves_the_rest_in_place() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = crates(&w, "a");
    let i = Instance::new(&a, &dir).unwrap().holding(templates());
    i.seed("seed the templates", &Refuse).unwrap();

    let tip = a.local_tip().unwrap().unwrap();
    let on_ref = git(&clone_path(&w, "a"), &["ls-tree", "-r", "--name-only", &tip]);
    assert_eq!(on_ref.lines().collect::<Vec<_>>(), ["one/DESIGN.md.tmpl", "two/README.md.tmpl"]);
    assert!(dir.join("one/src/lib.rs").is_file(), "a source stays where it was");
    assert!(!i.is_dirty().unwrap(), "a source beside the templates is no change of the instance");

    // the control: a template changed is a change, and a source changed is not
    write(&dir, "one/src/lib.rs", "pub fn one() -> u8 { 1 }\n");
    assert!(!i.is_dirty().unwrap());
    write(&dir, "one/DESIGN.md.tmpl", "design of one, again\n");
    assert!(i.is_dirty().unwrap());
}

#[test]
fn the_exclude_lines_hide_the_held_files_and_show_the_others() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = crates(&w, "a");
    Instance::new(&a, &dir).unwrap().holding(templates()).seed("seed", &Refuse).unwrap();

    let status = git(&clone_path(&w, "a"), &["status", "--porcelain", "-uall"]);
    assert!(!status.contains(".md.tmpl"), "a held file shows in the branch's status:\n{status}");
    assert!(status.contains("crates/one/src/lib.rs"), "the control: a source does show:\n{status}");
    let exclude = std::fs::read_to_string(clone_path(&w, "a").join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == "/crates/**/*.md.tmpl"), "{exclude}");
    assert!(!exclude.lines().any(|l| l == "/crates/"), "the whole directory is not excluded:\n{exclude}");
}

#[test]
fn materialize_fills_a_directory_that_holds_other_files_and_refuses_one_that_holds_held_ones() {
    let w = World::new();
    let a = store(&w, "a");
    let dir_a = crates(&w, "a");
    Instance::new(&a, &dir_a).unwrap().holding(templates()).seed("seed", &Refuse).unwrap();

    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("crates");
    write(&dir_b, "one/src/lib.rs", "pub fn one() {}\n");
    let i = Instance::new(&b, &dir_b).unwrap().holding(templates());
    i.materialize().unwrap();
    assert_eq!(std::fs::read_to_string(dir_b.join("one/DESIGN.md.tmpl")).unwrap(), "design of one\n");
    assert_eq!(std::fs::read_to_string(dir_b.join("two/README.md.tmpl")).unwrap(), "readme of two\n");
    assert!(dir_b.join("one/src/lib.rs").is_file());

    // a directory that already holds a held file is not filled over it
    let c = store(&w, "c");
    c.refresh().unwrap();
    let dir_c = clone_path(&w, "c").join("crates");
    write(&dir_c, "one/DESIGN.md.tmpl", "somebody's own\n");
    let err = Instance::new(&c, &dir_c).unwrap().holding(templates()).materialize().unwrap_err();
    assert!(matches!(err, StoreError::Instance { .. }), "{err}");
    assert_eq!(std::fs::read_to_string(dir_c.join("one/DESIGN.md.tmpl")).unwrap(), "somebody's own\n");
}

#[test]
fn a_change_travels_from_one_clone_to_another_and_touches_no_other_file() {
    let w = World::new();
    let a = store(&w, "a");
    let dir_a = crates(&w, "a");
    let ia = Instance::new(&a, &dir_a).unwrap().holding(templates());
    ia.seed("seed", &Refuse).unwrap();

    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir_b = clone_path(&w, "b").join("crates");
    write(&dir_b, "two/src/lib.rs", "b's own source\n");
    let ib = Instance::new(&b, &dir_b).unwrap().holding(templates());
    ib.materialize().unwrap();

    write(&dir_a, "one/DESIGN.md.tmpl", "design of one, second\n");
    std::fs::remove_file(dir_a.join("two/README.md.tmpl")).unwrap();
    write(&dir_a, "three/BACKLOG.md.tmpl", "backlog of three\n");
    ia.apply("edit", &Refuse).unwrap();

    ib.update().unwrap();
    assert_eq!(std::fs::read_to_string(dir_b.join("one/DESIGN.md.tmpl")).unwrap(), "design of one, second\n");
    assert!(!dir_b.join("two/README.md.tmpl").exists(), "the removal travelled");
    assert_eq!(std::fs::read_to_string(dir_b.join("three/BACKLOG.md.tmpl")).unwrap(), "backlog of three\n");
    assert_eq!(
        std::fs::read_to_string(dir_b.join("two/src/lib.rs")).unwrap(),
        "b's own source\n",
        "a file the instance does not hold is left as it was, and its directory stays"
    );
}

#[test]
fn a_ref_that_holds_a_file_outside_the_patterns_is_refused_and_nothing_is_written() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(
        &Edit::new().put("one/DESIGN.md.tmpl", "design\n").put("one/src/lib.rs", "smuggled\n"),
        "a writer that knew no pattern",
        &Reapply,
    )
    .unwrap();
    let dir = clone_path(&w, "a").join("crates");
    let err = Instance::new(&a, &dir).unwrap().holding(templates()).materialize().unwrap_err();
    let StoreError::Instance { paths, .. } = &err else { panic!("{err}") };
    assert_eq!(paths, &["one/src/lib.rs".to_owned()]);
    assert!(!dir.join("one/src/lib.rs").exists(), "a source was written from a data ref");
    assert!(!dir.join("one/DESIGN.md.tmpl").exists(), "the refusal wrote half of the tip");

    // the control: the same tip fills an instance that holds everything
    let all = clone_path(&w, "a").join("all");
    Instance::new(&a, &all).unwrap().materialize().unwrap();
    assert!(all.join("one/src/lib.rs").is_file());
}

#[test]
fn a_symlink_or_a_build_directory_outside_the_patterns_is_not_read() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = crates(&w, "a");
    #[cfg(unix)]
    std::os::unix::fs::symlink("src", dir.join("one/link")).unwrap();
    let i = Instance::new(&a, &dir).unwrap().holding(templates());
    i.seed("seed", &Refuse).expect("a symlink the patterns do not match is not the instance's");

    // the control: a symlink the patterns do match is refused, as in an instance of everything
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("DESIGN.md.tmpl", dir.join("one/ALIAS.md.tmpl")).unwrap();
        let err = i.edit().unwrap_err();
        assert!(matches!(err, StoreError::Instance { .. }), "{err}");
    }
}
