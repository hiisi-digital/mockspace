//! A file whose name is not UTF-8, as a tool writing Latin-1 leaves one. A ref names its files as
//! text, so it cannot hold one; what it must not do is refuse over a file it never holds (the
//! branch's own, beside a ref held by pattern, or ignored build output), and where it would hold one
//! it refuses naming the file, with nothing changed.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use refs_e2e::consumer::{self, NOT_UTF8};
use refs_e2e::whole::{self, Allowed};
use refs_e2e::{Clone, World};

fn put_odd(c: &Clone, dir: &str) {
    let p = c.path(dir).join(OsStr::from_bytes(NOT_UTF8));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"a name in latin-1\n").unwrap();
}

fn call(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// A consumer with no odd files, cloned, the engine's first run made.
fn plain(w: &World) -> Clone<'_> {
    consumer::LARGE.build(&w.first_clone("seed"));
    let c = w.clone_of("a");
    let _ = c.run(&["status"]);
    c
}

// Mutation: refuse a name that is not text wherever the walk meets it, as it did; the templates ref,
// whose instance lies among the branch's sources, then cannot be made or read while any source
// file under `crates/` has such a name, though the ref never holds it.
#[test]
fn a_branch_file_whose_name_is_not_utf8_does_not_stop_a_ref_held_by_pattern() {
    let w = World::new();
    let c = plain(&w);
    put_odd(&c, "mock/crates/alpha/src");
    c.commit_all("feat: a source with a latin-1 name");
    let before = whole::capture(&c);
    let args = consumer::move_args("templates", "crates", &["**/*.md.tmpl"]);
    let o = c.run(&call(&args));
    assert!(o.ok, "the ref holds templates only, and the odd name is a source's: {}", o.all());
    let after = whole::capture(&c);
    whole::kept(&c, &before, &after, &Allowed { rewritten: [b"mockspace.toml".to_vec()].into(), ..Allowed::default() }).unwrap();
    let tracked = c.git(&["ls-files", "-z", "--", "mock/crates/alpha/src"]);
    assert!(tracked.contains("caf"), "the branch still tracks the source: {tracked:?}");
    c.commit_all("chore: move `templates` to its ref");
    let s = c.ok(&["data-refs", "status", "templates"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "{}", s.all());
}

// Mutation: as above; the name is refused before the walk asks whether it is ignored, so build
// output with such a name stops every command on the instance.
#[test]
fn ignored_build_output_whose_name_is_not_utf8_does_not_stop_the_instance() {
    let w = World::new();
    let c = plain(&w);
    let args = consumer::move_args("research", "research", &[]);
    c.ok(&call(&args));
    c.commit_all("chore: move `research` to its ref");
    c.write("mock/research/sketch/target/CACHEDIR.TAG", refs_e2e::fixture::CACHE_TAG);
    put_odd(&c, "mock/research/sketch/target/debug");
    put_odd(&c, "mock/research/secrets");
    std::fs::write(c.path("mock/research").join(OsStr::from_bytes(b"out\xe9.o")), b"an object\n").unwrap();
    c.write(".gitignore", format!("{}secrets/\n*.o\n", String::from_utf8(c.read(".gitignore")).unwrap()).as_bytes());
    let s = c.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "{}", s.all());
    assert!(s.stdout.contains("ignored, not held: secrets/"), "{}", s.all());
}

// Mutation: refuse without the path, as it did; in a directory of thousands of files the person
// cannot find which name stops the move.
#[test]
fn a_file_the_ref_would_hold_whose_name_is_not_utf8_is_refused_by_name_and_nothing_changes() {
    let w = World::new();
    let c = plain(&w);
    put_odd(&c, "mock/research/notes");
    c.commit_all("docs: a note with a latin-1 name");
    let before = whole::capture(&c);
    let args = consumer::move_args("research", "research", &[]);
    let o = c.run(&call(&args));
    assert!(!o.ok, "{}", o.all());
    assert!(o.all().contains("not text") && o.all().contains("notes/caf"), "the refusal names the file: {}", o.all());
    whole::kept(&c, &before, &whole::capture(&c), &Allowed::default()).unwrap();
    assert!(w.remote_tip("refs/heads/mock/research").is_none(), "no ref was made");
    // the way out: give it a name in UTF-8 on the branch, and the same call goes through
    let from = c.path("mock/research/notes").join(OsStr::from_bytes(NOT_UTF8));
    std::fs::rename(&from, c.path("mock/research/notes/cafe.md")).unwrap();
    c.commit_all("docs: rename the latin-1 note");
    c.ok(&call(&args));
    assert_eq!(c.read("mock/research/notes/cafe.md"), b"a name in latin-1\n");
}
