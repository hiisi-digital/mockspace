//! A rebase across the migration commit, a symlink where `update` and `restore` write, a ref that
//! takes the mock directory itself, and two clones making the same new ref at once.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{World, fixture};

use refs_e2e::fixture::NOTES;

// Mutation: drop the sparse block, so the checkout of the old branch writes its bytes over the
// instance file and the rebase then replays onto them; the instance's bytes are gone from disk and
// the next commit pushes the old branch's.
#[test]
fn a_rebase_of_a_branch_cut_before_the_migration_is_refused_and_the_instance_file_kept() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.git(&["checkout", "-q", "-b", "feat"]);
    a.write(NOTES, b"edited on the old branch\n");
    a.commit_all("docs: edit the notes on the branch");
    a.git(&["checkout", "-q", "dev"]);
    fixture::migrate_all(&a);
    a.push();
    let instance = a.read(NOTES);
    assert_ne!(instance, b"edited on the old branch\n", "the control: the old branch holds other bytes");
    a.git(&["checkout", "-q", "feat"]);
    assert_eq!(a.read(NOTES), instance, "the checkout of the old branch wrote over the instance file");
    let rb = a.git_out(&["rebase", "dev"]);
    assert!(!rb.ok && rb.all().contains("unstaged changes"), "git took the instance file for its own: {}", rb.all());
    assert_eq!(a.read(NOTES), instance, "the refused rebase wrote over the instance file");
}

// Mutation: write through a symlink in `update`; the file lands outside the instance.
#[test]
fn update_refuses_a_symlink_in_the_instance_and_writes_nothing() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    let outside = w.base().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, a.path("mock/research/jump")).unwrap();
    b.write("mock/research/jump/pwn.md", b"through the link\n");
    b.write(NOTES, b"b changed the notes\n");
    b.ok(&["data-refs", "commit", "research"]);
    let before = a.read(NOTES);
    let o = a.fails(&["data-refs", "update", "research"]);
    assert!(o.all().contains("symlink") && o.all().contains("jump"), "the refusal names the link: {}", o.all());
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "a write went through the link");
    assert_eq!(a.read(NOTES), before, "the update wrote part of the tip before it refused");
}

// Mutation: write through a symlink in `restore`.
#[test]
fn restore_refuses_a_symlink_where_a_directory_was_and_writes_nothing() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let outside = w.base().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::remove_dir_all(a.path("mock/research/20261001_alpha")).unwrap();
    std::os::unix::fs::symlink(&outside, a.path("mock/research/20261001_alpha")).unwrap();
    a.remove("mock/research/empty.txt");
    let o = a.fails(&["data-refs", "restore", "research"]);
    assert!(o.all().contains("symlink") && o.all().contains("20261001_alpha"), "the refusal names the link: {}", o.all());
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "a write went through the link");
    assert!(!a.exists("mock/research/empty.txt"), "the restore wrote part of the base before it refused");
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

fn declare_extra(a: &refs_e2e::Clone<'_>) {
    let cfg = String::from_utf8(a.read("mock/mockspace.toml")).unwrap();
    a.write("mock/mockspace.toml", format!("{cfg}\n[[data_ref]]\nnamespace = \"extra\"\npath = \"extra\"\n").as_bytes());
    a.git(&["add", "-f", "--", "mock/mockspace.toml"]);
    a.commit_all("chore: declare `extra`");
    a.push();
}

// Mutation: let `instantiate` of a declared ref the remote has not got yet fail, or write a record
// the standing gate does not take, so the first commit that makes the ref has no way to run.
#[test]
fn a_declared_ref_not_born_yet_instantiates_empty_and_passes_the_gate() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::migrate_one(&a, "research", "research", &[]);
    declare_extra(&a);
    let out = a.ok(&["data-refs", "instantiate", "extra"]);
    assert!(out.stdout.contains("not born"), "{}", out.all());
    let made = std::fs::read_dir(a.path("mock/extra")).map(|d| d.count());
    assert_eq!(made.ok(), Some(0), "the instance is an empty directory");
    a.ok(&["data-refs", "verify", "extra"]);
    // the standing gate takes it: a command that reads the mock directory runs
    a.ok(&["data-refs", "status", "extra"]);
    assert!(w.remote_tip("refs/heads/mock/extra").is_none(), "instantiate creates no ref");
    a.write("mock/extra/first.md", b"first\n");
    a.ok(&["data-refs", "commit", "extra"]);
    assert!(w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/extra"]).contains("first.md"));
    a.ok(&["data-refs", "verify", "extra"]);
}

// Mutation: let the late clone's first commit be refused for good, or overwrite the early file.
#[test]
fn the_late_clone_of_a_ref_not_born_yet_merges_after_its_push_is_rejected() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::migrate_one(&a, "research", "research", &[]);
    declare_extra(&a);
    let b = w.clone_of("b");
    b.ok(&["data-refs", "instantiate", "research"]);
    a.ok(&["data-refs", "instantiate", "extra"]);
    b.ok(&["data-refs", "instantiate", "extra"]);
    a.write("mock/extra/from-a.md", b"a\n");
    b.write("mock/extra/from-b.md", b"b\n");
    a.ok(&["data-refs", "commit", "extra"]);
    let late = b.run(&["data-refs", "commit", "extra"]);
    if !late.ok {
        b.ok(&["data-refs", "update", "extra"]);
        b.ok(&["data-refs", "commit", "extra"]);
    }
    let tree = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/extra"]);
    assert!(tree.contains("from-a.md") && tree.contains("from-b.md"), "{tree}");
    assert_eq!(b.read("mock/extra/from-a.md"), b"a\n");
    assert_eq!(b.read("mock/extra/from-b.md"), b"b\n");
    b.ok(&["data-refs", "verify", "extra"]);
}

// Control: only a declared ref is not born by instantiate; a namespace nobody declared is refused.
#[test]
fn a_ref_absent_from_the_declarations_is_still_refused_by_instantiate() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::migrate_one(&a, "research", "research", &[]);
    let out = a.fails(&["data-refs", "instantiate", "nowhere"]);
    assert!(out.all().contains("no [[data_ref]] has the namespace `nowhere`"), "{}", out.all());
    assert!(!a.exists("mock/nowhere"));
}
