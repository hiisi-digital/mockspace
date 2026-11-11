//! A full migration of the generated consumer, through the real command entry.

mod refs_e2e;

use refs_e2e::{check, content_only, fixture, held_on_remote, instance_matches, no_loss, World};

const CONFIG: &[&str] = &["mock/mockspace.toml"];

// Mutation: make `directory-to-ref` skip one file of the held set when it seeds the ref, or give
// the ref a changed blob; the check then reports that path as lost or changed.
#[test]
fn a_full_migration_loses_no_content_and_no_path() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build(&a);
    let before = check::at(&a, "HEAD");
    fixture::migrate_all(&a);
    let held = held_on_remote(&w, &fixture::decls());
    assert!(held.len() >= 20, "the refs hold the held set: {}", held.len());
    no_loss(&content_only(&before), &content_only(&check::branch(&a)), &content_only(&held), CONFIG).unwrap();
    instance_matches(&a, &held, false).unwrap();
}

// Mutation: write every ref entry as 100644, as `ref_ops` does, which is the defect this catches.
// Passing needs the executable bit to survive the move to the ref and back to disk.
#[test]
fn a_full_migration_keeps_the_executable_bit() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build(&a);
    let before = check::at(&a, "HEAD");
    fixture::migrate_all(&a);
    let held = held_on_remote(&w, &fixture::decls());
    no_loss(&before, &check::branch(&a), &held, CONFIG).unwrap();
    instance_matches(&a, &held, true).unwrap();
}

// Mutation: a migration that appends the table to the wrong file; the branch then does not hold
// the config the clones read, and the second clone cannot instantiate.
#[test]
fn a_fresh_clone_instantiates_and_sees_the_same_bytes() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build(&a);
    fixture::migrate_all(&a);
    a.push();
    let b = w.clone_of("b");
    // before the instance exists, every other command stops
    let blocked = b.fails(&["--generate-only"]);
    assert!(blocked.all().contains("BLOCKED"), "{}", blocked.all());
    b.ok(&["data-refs", "instantiate"]);
    let held = held_on_remote(&w, &fixture::decls());
    instance_matches(&b, &held, false).unwrap();
    // every file the first clone had on disk under the held directories is the second clone's
    for p in held.keys() {
        assert_eq!(a.read(p), b.read(p), "{p}");
    }
    assert!(b.untracked().iter().all(|p| !p.starts_with("mock/research/")), "{:?}", b.untracked());
    b.ok(&["data-refs", "verify"]);
}

// Mutation: seed the ref from a walk of the disk instead of the files the branch tracks; the
// ignored `target/` directories inside the held directories then land on the remote.
#[test]
fn ignored_build_output_inside_a_held_directory_is_not_put_on_the_ref() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build(&a);
    let before = check::at(&a, "HEAD");
    fixture::migrate_all(&a);
    let held = held_on_remote(&w, &fixture::decls());
    assert_eq!(refs_e2e::strays(&before, &held), Vec::<String>::new());
}
