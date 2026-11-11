//! A rebase across the migration commit, a symlink where `update` and `restore` write, a ref that
//! takes the mock directory itself, and two clones making the same new ref at once.

mod refs_e2e;

use refs_e2e::{Held, World, fixture, held_on_remote, instance_matches};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";

// Mutation: let the engine drop the instance when a rebase replays a commit that edits a path the
// migration took off the branch; the edit would exist in no place.
#[test]
fn a_rebase_of_a_branch_cut_before_the_migration_loses_no_edit() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.git(&["checkout", "-q", "-b", "feat"]);
    a.write(NOTES, b"edited on the old branch\n");
    a.commit_all("docs: edit the notes on the branch");
    let on_branch = a.git(&["rev-parse", "HEAD"]).trim().to_string();
    a.git(&["checkout", "-q", "dev"]);
    fixture::migrate_all(&a);
    a.push();
    let instance = a.read(NOTES);
    a.git(&["checkout", "-q", "feat"]);
    let _ = a.git_out(&["rebase", "dev"]);
    let _ = a.git_out(&["rebase", "--abort"]);
    // the edit is in the old commit whatever the rebase did, and the instance is not the casualty
    let text = a.git(&["show", &format!("{on_branch}:{NOTES}")]);
    assert_eq!(text.as_bytes(), b"edited on the old branch\n");
    assert!(a.exists(NOTES), "the instance file is still on disk");
    assert!(instance == a.read(NOTES) || a.read(NOTES) == b"edited on the old branch\n");
}

// Mutation: write through a symlink in `update`; the file lands outside the instance.
#[test]
fn update_does_not_write_below_a_symlink_in_the_instance() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    let outside = w.base().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, a.path("mock/research/jump")).unwrap();
    b.write("mock/research/jump/pwn.md", b"through the link\n");
    b.ok(&["data-refs", "commit", "research"]);
    let _ = a.run(&["data-refs", "update", "research"]);
    assert!(!outside.join("pwn.md").exists(), "a write went through the link");
}

// Mutation: write through a symlink in `restore`.
#[test]
fn restore_does_not_write_below_a_symlink_that_stands_where_a_directory_was() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let outside = w.base().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::remove_dir_all(a.path("mock/research/20261001_alpha")).unwrap();
    std::os::unix::fs::symlink(&outside, a.path("mock/research/20261001_alpha")).unwrap();
    let _ = a.run(&["data-refs", "restore", "research"]);
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "a write went through the link");
}

// Mutation: treat `path = "."` as a plain directory; the patterns then reach the instance's own
// config and registry, or the round trip loses a file.
#[test]
fn a_ref_on_the_mock_directory_itself_round_trips_to_a_second_clone() {
    let w = World::new();
    {
        let a = w.first_clone("a");
        fixture::build_tracked(&a);
        fixture::migrate_one(&a, "mockdir", ".", &["research/**", "benches/**"]);
        a.push();
    }
    let a = w.clone_of("a2");
    a.ok(&["data-refs", "instantiate"]);
    a.write(NOTES, b"changed in the second clone\n");
    a.write("mock/research/new.md", b"new\n");
    a.ok(&["data-refs", "commit", "mockdir"]);
    let b = w.clone_of("b");
    b.ok(&["data-refs", "instantiate"]);
    assert_eq!(b.read(NOTES), b"changed in the second clone\n");
    assert_eq!(b.read("mock/research/new.md"), b"new\n");
    b.ok(&["data-refs", "verify"]);
    assert!(b.tracked().iter().all(|p| !p.starts_with("mock/research/")), "{:?}", b.tracked());
}

// RED, a new defect: a ref declared after the migration and absent on the remote cannot be
// instantiated (the absent ref is not read as empty), nor committed (no instance), so the first
// commit the guide says makes the ref has no way to run.
// Mutation: let the late clone's first commit overwrite the ref the early one made.
#[test]
fn two_clones_making_the_same_new_ref_lose_neither_file() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::migrate_one(&a, "research", "research", &[]);
    let cfg = String::from_utf8(a.read("mock/mockspace.toml")).unwrap();
    a.write("mock/mockspace.toml", format!("{cfg}\n[[data_ref]]\nnamespace = \"extra\"\npath = \"extra\"\n").as_bytes());
    a.git(&["add", "-f", "--", "mock/mockspace.toml"]);
    a.commit_all("chore: declare `extra`");
    a.push();
    let b = w.clone_of("b");
    b.ok(&["data-refs", "instantiate", "research"]);
    a.ok(&["data-refs", "instantiate", "extra"]);
    b.ok(&["data-refs", "instantiate", "extra"]);
    for (c, file) in [(&a, "from-a.md"), (&b, "from-b.md")] {
        c.write(&format!("mock/extra/{file}"), file.as_bytes());
    }
    a.ok(&["data-refs", "commit", "extra"]);
    let late = b.run(&["data-refs", "commit", "extra"]);
    let tree = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/extra"]);
    assert!(tree.contains("from-a.md"), "the early commit stays: {tree}");
    assert!(b.exists("mock/extra/from-b.md"), "the late edit is still on disk: {}", late.all());
    if late.ok {
        assert!(tree.contains("from-b.md"), "a success holds the late file: {tree}");
    }
}
