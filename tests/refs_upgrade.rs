//! Clones that exist before the migration lands on `dev`, and pull it.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::collections::BTreeMap;

use refs_e2e::{Held, World, fixture, held_on_remote, instance_matches, status_of};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const ALL: [Held; 4] = [
    Held { ns: "research", path: "research" },
    Held { ns: "benches", path: "benches" },
    Held { ns: "rounds", path: "design_rounds" },
    Held { ns: "templates", path: "crates" },
];

/// A remote on which `dev` is pre-migration, a clone `old` of it, and the migration then pushed.
fn before_the_migration_lands(w: &World) -> refs_e2e::Clone<'_> {
    {
        let seed = w.first_clone("seed");
        fixture::build_tracked(&seed);
    }
    w.clone_of("old")
}

fn the_migration_lands(w: &World) {
    let seed = w.clone_of("seed2");
    fixture::migrate_all(&seed);
    seed.push();
}

/// Every file of the held directories as the clone has it on disk.
fn snapshot(c: &refs_e2e::Clone, paths: &[String]) -> BTreeMap<String, Vec<u8>> {
    paths.iter().filter(|p| c.exists(p)).map(|p| (p.clone(), c.read(p))).collect()
}

// Mutation: have the migration's removal be a deletion git carries out on a pull without the data
// being anywhere else; the instantiate below would find nothing for the files and the comparison
// with the snapshot fails.
#[test]
fn a_clean_clone_pulls_the_migration_and_instantiates_every_file_it_had() {
    let w = World::new();
    let old = before_the_migration_lands(&w);
    let held_paths: Vec<String> = old
        .tracked()
        .into_iter()
        .filter(|p| ["mock/research/", "mock/benches/", "mock/design_rounds/2", "mock/crates/alpha/README.md.tmpl", "mock/crates/alpha/docs/", "mock/crates/beta/README.md.tmpl", "mock/crates/beta/docs/"].iter().any(|s| p.starts_with(s)))
        .collect();
    let before = snapshot(&old, &held_paths);
    the_migration_lands(&w);
    old.git(&["pull", "-q", "--ff-only", "origin", "dev"]);
    let blocked = old.fails(&["--generate-only"]);
    assert!(blocked.all().contains("instantiate"), "{}", blocked.all());
    old.ok(&["data-refs", "instantiate"]);
    assert_eq!(snapshot(&old, &held_paths), before, "every file the clone had is back, byte for byte");
    instance_matches(&old, &held_on_remote(&w, &ALL), false).unwrap();
    old.ok(&["--generate-only"]);
}

// Mutation: let the pull overwrite a modified file with the deletion; the edit would be gone.
#[test]
fn an_uncommitted_edit_in_a_file_the_migration_removes_survives_the_pull() {
    let w = World::new();
    let old = before_the_migration_lands(&w);
    old.write(NOTES, b"uncommitted edit\n");
    the_migration_lands(&w);
    let _ = old.git_out(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    assert_eq!(old.read(NOTES), b"uncommitted edit\n");
}

// Mutation: have `instantiate` write the ref's file over one that is there; the edit below would
// be replaced by the ref's older bytes.
#[test]
fn a_committed_local_edit_meets_the_migration_as_a_conflict_and_goes_on_to_the_ref() {
    let w = World::new();
    let old = before_the_migration_lands(&w);
    old.write(NOTES, b"local committed edit\n");
    old.commit_all("docs: a local edit");
    the_migration_lands(&w);
    let o = old.git_out(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    assert!(!o.ok, "a modify against a delete is a conflict: {}", o.all());
    assert_eq!(old.read(NOTES), b"local committed edit\n", "the conflicted file keeps the edit");
    // resolve as the migration did: the branch no longer tracks it, the file stays
    old.git(&["rm", "-q", "--cached", "--sparse", NOTES]);
    old.commit_staged("merge: take the migration");
    // the directory is not empty, so `instantiate` refuses it and touches nothing of it
    let o = old.run(&["data-refs", "instantiate", "research"]);
    assert!(!o.ok, "{}", o.all());
    assert_eq!(old.read(NOTES), b"local committed edit\n", "the refusal leaves the edit: {}", o.all());
    // setting the edit aside and instantiating is the way through, and the edit goes back on top
    let aside = old.read(NOTES);
    old.remove("mock/research");
    old.ok(&["data-refs", "instantiate", "research"]);
    assert!(old.exists("mock/research/crlf.txt"), "the other files arrive");
    for ns in ["benches", "rounds", "templates"] {
        old.ok(&["data-refs", "instantiate", ns]);
    }
    old.write(NOTES, &aside);
    old.ok(&["data-refs", "commit", "research"]);
    let held = held_on_remote(&w, &ALL);
    assert_eq!(refs_e2e::check::blob_id(&old, NOTES), held[NOTES].1, "the edit is on the ref");
    assert!(status_of(&old, "research").ends_with("clean"));
}
