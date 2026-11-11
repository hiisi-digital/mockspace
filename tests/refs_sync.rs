//! Two clones following the same refs, through `data-refs update` and `commit`.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{fixture, status_of, tip};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const SPACES: &str = "mock/research/notes with spaces.md";
const EMPTY: &str = "mock/research/empty.txt";

// Mutation: make `update` overwrite every file with the tip's; the clone's own uncommitted edit
// is then replaced and the first assertion fails.
#[test]
fn an_update_brings_in_the_other_clones_change_and_keeps_an_edit_of_another_file() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.write(NOTES, b"a's edit, not committed\n");
    b.write(SPACES, b"b's edit\n");
    b.write("mock/research/new.md", b"new by b\n");
    b.remove(EMPTY);
    b.ok(&["data-refs", "commit", "research"]);
    a.ok(&["data-refs", "update", "research"]);
    assert_eq!(a.read(NOTES), b"a's edit, not committed\n");
    assert_eq!(a.read(SPACES), b"b's edit\n");
    assert_eq!(a.read("mock/research/new.md"), b"new by b\n");
    assert!(!a.exists(EMPTY), "b's deletion arrives");
    assert!(status_of(&a, "research").ends_with("changed"), "{}", status_of(&a, "research"));
    // and the kept edit is still part of a's next commit, which lands on top of b's
    a.ok(&["data-refs", "commit", "research"]);
    let c = fixture::seat(&w, "c");
    assert_eq!(c.read(NOTES), b"a's edit, not committed\n");
    assert_eq!(c.read(SPACES), b"b's edit\n");
    assert!(!c.exists(EMPTY));
}

// Mutation: let `update` write the tip over a file both sides changed.
#[test]
fn a_file_changed_on_both_sides_is_refused_and_nothing_is_written() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.write(NOTES, b"a's edit\n");
    a.write("mock/research/other.md", b"a's other file\n");
    b.write(NOTES, b"b's edit\n");
    b.write("mock/research/from-b.md", b"b's new file\n");
    b.ok(&["data-refs", "commit", "research"]);
    let before = tip(&w, "research");
    let o = a.fails(&["data-refs", "update", "research"]);
    assert!(o.all().contains("NOTES.md"), "the refusal names the file: {}", o.all());
    assert_eq!(a.read(NOTES), b"a's edit\n");
    assert!(!a.exists("mock/research/from-b.md"), "nothing is written on a refusal");
    assert_eq!(a.read("mock/research/other.md"), b"a's other file\n");
    assert_eq!(tip(&w, "research"), before);
}

// Mutation: treat a deletion as no change, so the tip's edit lands over the deleted file.
#[test]
fn a_file_deleted_here_and_edited_there_is_refused() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.remove(NOTES);
    b.write(NOTES, b"b's edit\n");
    b.ok(&["data-refs", "commit", "research"]);
    let o = a.run(&["data-refs", "update", "research"]);
    assert!(!o.ok, "a deletion against an edit is a change on both sides: {}", o.all());
    assert!(!a.exists(NOTES), "the deletion is not undone silently");
}

// Mutation: treat a file edited here and deleted there as unchanged here, so it is deleted.
#[test]
fn a_file_edited_here_and_deleted_there_keeps_the_edit() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.write(NOTES, b"a's edit\n");
    b.remove(NOTES);
    b.ok(&["data-refs", "commit", "research"]);
    let o = a.run(&["data-refs", "update", "research"]);
    assert!(a.exists(NOTES) && a.read(NOTES) == b"a's edit\n", "the edit survives, ok={}: {}", o.ok, o.all());
}

#[test]
fn a_file_deleted_on_both_sides_is_no_conflict() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.remove(EMPTY);
    b.remove(EMPTY);
    b.ok(&["data-refs", "commit", "research"]);
    a.ok(&["data-refs", "update", "research"]);
    assert!(!a.exists(EMPTY));
}

// Mutation: let `update` write back every file the tip holds, undoing the local deletion.
#[test]
fn a_local_deletion_survives_an_update_and_goes_with_the_next_commit() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.remove(EMPTY);
    b.write(SPACES, b"b's edit\n");
    b.ok(&["data-refs", "commit", "research"]);
    a.ok(&["data-refs", "update", "research"]);
    assert!(!a.exists(EMPTY), "the deletion is kept, not restored by the update");
    a.ok(&["data-refs", "commit", "research"]);
    let c = fixture::seat(&w, "c");
    assert!(!c.exists(EMPTY));
    assert_eq!(c.read(SPACES), b"b's edit\n");
}

fn mode_of(c: &refs_e2e::Clone, rel: &str) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(c.path(rel)).unwrap().permissions().mode() & 0o111
}

fn set_exec(c: &refs_e2e::Clone, rel: &str, on: bool) {
    let p = c.path(rel);
    let body = std::fs::read(&p).unwrap();
    if on {
        mockspace_manifest::write_executable(&p, body).unwrap();
    } else {
        // A file made new takes the plain mode, so nothing here sets one.
        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, body).unwrap();
    }
}

// Mutation: compare content ids only, as before the mode was part of an id; the chmod below is then
// not seen by `status`, and the second clone never gets it.
#[test]
fn a_mode_change_alone_is_seen_committed_and_round_trips_to_a_second_clone() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    let plain = "mock/research/crlf.txt";
    let script = "mock/benches/hot/run.sh";
    assert_eq!(mode_of(&b, plain), 0);
    assert_ne!(mode_of(&b, script), 0, "an executable arrives executable");
    set_exec(&a, plain, true);
    set_exec(&a, script, false);
    assert!(status_of(&a, "research").ends_with("changed"), "{}", status_of(&a, "research"));
    assert!(status_of(&a, "benches").ends_with("changed"), "{}", status_of(&a, "benches"));
    a.ok(&["data-refs", "commit"]);
    let tree = w.remote_git(&["ls-tree", "refs/heads/mock/research", "crlf.txt"]);
    assert!(tree.starts_with("100755"), "{tree}");
    let tree = w.remote_git(&["ls-tree", "refs/heads/mock/benches", "hot/run.sh"]);
    assert!(tree.starts_with("100644"), "{tree}");
    b.ok(&["data-refs", "update"]);
    assert_ne!(mode_of(&b, plain), 0, "the mode alone arrived");
    assert_eq!(mode_of(&b, script), 0, "and the mode taken away arrived");
    assert!(status_of(&b, "research").ends_with("clean"), "{}", status_of(&b, "research"));
    let c = fixture::seat(&w, "c");
    assert_ne!(mode_of(&c, plain), 0);
}

// Mutation: restore writes files with the default mode.
#[test]
fn restore_gives_a_file_back_its_executable_bit() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let script = "mock/benches/hot/run.sh";
    a.remove(script);
    a.ok(&["data-refs", "restore", "benches"]);
    assert_ne!(mode_of(&a, script), 0);
}

// Mutation: walk the instance without the ignore rules; build output written after the migration
// would then be committed to the ref and `status` would call the instance changed.
#[test]
fn build_output_written_into_an_instance_is_neither_changed_nor_committed() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    fixture::add_ignored_output(&a);
    assert!(status_of(&a, "research").ends_with("clean"), "{}", status_of(&a, "research"));
    a.write(NOTES, b"an edit\n");
    a.ok(&["data-refs", "commit", "research"]);
    let listing = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"]);
    assert!(!listing.contains("target/"), "{listing}");
    assert!(listing.contains("20261001_alpha/NOTES.md"));
}
