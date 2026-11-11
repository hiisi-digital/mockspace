//! What an instance command does when git fails part way, and what a gated run costs in proportion
//! to the bytes of the instance. The failures and the counts are the spy's (`refs_e2e/spy.rs`), so
//! a failure is real and happens at one chosen step.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::time::Instant;

use refs_e2e::spy::{self, Spy};
use refs_e2e::{World, fixture};

use refs_e2e::fixture::NOTES;

fn remote_notes(w: &World) -> String {
    w.remote_git(&["show", "refs/heads/mock/research:20261001_alpha/NOTES.md"])
}

/// The calls of `commit`'s own, each told apart from the call of the same name the copy of the
/// edit makes: the commit's index is the store's, its message is the ref's, and it moves the ref
/// and not the copy.
const COMMIT_STEPS: [&str; 5] = [
    "write-tree mockspace-store",
    "commit-tree data: research",
    "update-ref !mock-backup",
    "push",
    "fetch",
];

/// The calls of the copy that `commit` makes of the edit first.
const COPY_STEPS: [&str; 4] =
    ["write-tree mockspace-backup", "commit-tree backup of", "update-ref mock-backup", "hash-object -w --no-filters --stdin-paths"];

/// A commit with the step `fail` failing: the step is reached, the edit is on disk after it, and
/// the commit made again (with `update` first where the commit landed and the local ref did not
/// follow) lands it once. Says whether the commit with the failure succeeded.
fn commit_failing_at(fail_at: &str) -> bool {
    let w = fixture::migrated_remote();
    let spy = spy::install(&w);
    let a = fixture::seat(&w, "a");
    a.write(NOTES, b"an edit that must survive\n");
    a.write("mock/research/new.md", b"a new file\n");
    let before = w.remote_tip("refs/heads/mock/research").unwrap();
    std::fs::write(&spy.fail, format!("{fail_at}\n")).unwrap();
    let o = a.run(&["data-refs", "commit", "research"]);
    spy.stop_failing();
    assert!(spy.calls("INJECTED ") > 0, "{fail_at}: the step was never reached, so nothing was tested: {}", o.all());
    assert_eq!(a.read(NOTES), b"an edit that must survive\n", "{fail_at}: the edit is on disk");
    assert_eq!(a.read("mock/research/new.md"), b"a new file\n", "{fail_at}: the new file is on disk");
    let again = a.run(&["data-refs", "commit", "research"]);
    if !again.ok {
        assert!(again.all().contains("data-refs update"), "{fail_at}: a refusal with no way on: {}", again.all());
        a.ok(&["data-refs", "update", "research"]);
        a.ok(&["data-refs", "commit", "research"]);
    }
    assert_eq!(a.read(NOTES), b"an edit that must survive\n", "{fail_at}: after the recovery");
    assert_eq!(remote_notes(&w), "an edit that must survive\n", "{fail_at}");
    let n = w.remote_git(&["rev-list", "--count", "refs/heads/mock/research"]);
    let was = w.remote_git(&["rev-list", "--count", &before]);
    assert_eq!(n.trim().parse::<u32>().unwrap(), was.trim().parse::<u32>().unwrap() + 1, "{fail_at}: committed once");
    o.ok
}

// Mutation: write the edit's commit to the remote ahead of finishing locally, or clear the edit
// before the commit is made; a failure then loses the edit or leaves two versions of it.
#[test]
fn a_commit_that_fails_at_any_of_its_own_steps_keeps_the_edit_and_the_retry_commits_it_once() {
    for step in COMMIT_STEPS {
        // a failure of the push or of the fetch before it may be one the store takes back, so only
        // a step past the push is asked to be said
        let ok = commit_failing_at(step);
        if matches!(step, "write-tree mockspace-store" | "commit-tree data: research") {
            assert!(!ok, "{step}: the commit was told to fail and said it succeeded");
        }
    }
}

// Mutation: let a failed copy of the edit fail the commit, or skip the commit's own steps after it;
// the copy is a safety net, and the commit is what was asked.
#[test]
fn a_copy_of_the_edit_that_fails_does_not_stop_the_commit() {
    for step in COPY_STEPS {
        assert!(commit_failing_at(step), "{step}: a failed copy failed the commit");
    }
}

// Mutation: remove the files the tip removed and fail to write the rest with nothing recorded and the
// next run not taking up where it stopped.
#[test]
fn an_update_that_fails_while_reading_blobs_loses_no_local_edit_and_the_retry_converges() {
    let w = fixture::migrated_remote();
    let spy = spy::install(&w);
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    b.write(NOTES, b"b changed the notes\n");
    b.write("mock/research/from-b.md", b"new from b\n");
    b.remove("mock/research/empty.txt");
    b.ok(&["data-refs", "commit", "research"]);
    a.write("mock/research/crlf.txt", b"a's own edit\n");
    std::fs::write(&spy.fail, "cat-file\n").unwrap();
    let o = a.run(&["data-refs", "update", "research"]);
    spy.stop_failing();
    assert!(spy.calls("INJECTED cat-file") > 0, "the read of the blobs was never reached: {}", o.all());
    assert!(!o.ok, "{}", o.all());
    assert_eq!(a.read("mock/research/crlf.txt"), b"a's own edit\n", "a local edit survives the failure");
    a.ok(&["data-refs", "update", "research"]);
    assert_eq!(a.read(NOTES), b"b changed the notes\n");
    assert_eq!(a.read("mock/research/from-b.md"), b"new from b\n");
    assert!(!a.exists("mock/research/empty.txt"));
    assert_eq!(a.read("mock/research/crlf.txt"), b"a's own edit\n", "and after the retry");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "the local edit is still an edit: {}", s.all());
}

fn big_instance(files: usize) -> World {
    let w = World::new();
    let a = w.first_clone("seed");
    fixture::build_tracked(&a);
    for i in 0..files {
        a.write(&format!("mock/research/bulk/{:03}/f{i:05}.md", i % 200), format!("file number {i}\n").repeat(8).as_bytes());
    }
    a.commit_all("docs: bulk research");
    fixture::migrate_one(&a, "research", "research", &[]);
    a.push();
    w
}

/// Every call that reads files to name their bytes: the hashing of `status` and of the copy. The
/// one `hash-object` of a few bytes that names an instance's record is not one.
const HASHING: &str = "--no-filters --stdin-paths";

/// Two runs of `args` with the margin waited out before them; asserts the first one hashed and
/// the second hashed nothing and wrote no copy.
fn second_run_hashes_nothing(a: &refs_e2e::Clone, spy: &Spy, args: &[&str]) {
    // an entry is trusted once the file was older than the margin when it was hashed
    std::thread::sleep(std::time::Duration::from_millis(2100));
    spy.clear();
    let t0 = Instant::now();
    a.ok(args);
    let first = t0.elapsed();
    assert!(spy.calls(HASHING) > 0, "the control: the first run hashes, or this counts the wrong call");
    spy.clear();
    let t1 = Instant::now();
    a.ok(args);
    let second = t1.elapsed();
    eprintln!("an ad-hoc spike, test profile, {args:?}: first {first:?}, second {second:?}");
    let log_text = spy.text();
    assert_eq!(spy.calls(HASHING), 0, "the second run hashed again: {log_text}");
    assert_eq!(spy.calls("commit-tree"), 0, "the second run wrote a copy of an unchanged edit: {log_text}");
}

// Mutation: hash every file of the instance on every run, as the first version did: the second
// run of an unchanged instance then makes as many `hash-object` calls as the first.
#[test]
fn an_unchanged_instance_is_not_hashed_again_on_the_next_run() {
    let w = big_instance(1500);
    let spy = spy::install(&w);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    second_run_hashes_nothing(&a, &spy, &["data-refs", "status", "research"]);
}

// Mutation: hash the edited files for their copy on every run, ahead of asking whether the copy
// already holds them; a dirty instance then costs a hash of every edited byte on every command.
#[test]
fn an_unchanged_edit_is_not_hashed_again_for_its_copy() {
    let w = big_instance(300);
    let spy = spy::install(&w);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    a.write(NOTES, &fixture::bytes(9, 200_000));
    a.write("mock/research/bulk/000/f00000.md", b"edited\n");
    a.write("mock/research/added.md", b"added\n");
    second_run_hashes_nothing(&a, &spy, &["data-refs", "status", "research"]);
}

// Mutation: the same for an instance of a ref not born yet, whose every file is an edit.
#[test]
fn an_instance_of_a_ref_not_born_yet_is_not_hashed_again() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::migrate_one(&a, "research", "research", &[]);
    let cfg = String::from_utf8(a.read("mock/mockspace.toml")).unwrap();
    a.write("mock/mockspace.toml", format!("{cfg}\n[[data_ref]]\nnamespace = \"extra\"\npath = \"extra\"\n").as_bytes());
    a.git(&["add", "-f", "--", "mock/mockspace.toml"]);
    a.commit_all("chore: declare `extra`");
    a.push();
    let spy = spy::install(&w);
    a.ok(&["data-refs", "instantiate", "extra"]);
    for i in 0 .. 50 {
        a.write(&format!("mock/extra/f{i}.md"), format!("file {i}\n").as_bytes());
    }
    second_run_hashes_nothing(&a, &spy, &["data-refs", "status", "extra"]);
}

// Mutation: trust the cache on the size alone; an edit of the same length is then never seen.
#[test]
fn an_edit_of_the_same_size_straight_after_a_run_is_still_seen() {
    let w = big_instance(20);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "status", "research"]);
    let same = a.read(NOTES).len();
    let edited: Vec<u8> = vec![b'x'; same];
    a.write(NOTES, &edited);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}

// Mutation: trust the cache on the size and the modification time; an edit that puts the old time
// back is then never seen. The change time is what a program cannot put back.
#[test]
fn an_edit_of_the_same_size_with_the_old_modification_time_put_back_is_still_seen() {
    let w = big_instance(20);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    std::thread::sleep(std::time::Duration::from_millis(2100));
    a.ok(&["data-refs", "status", "research"]);
    let p = a.path(NOTES);
    let old = std::fs::metadata(&p).unwrap().modified().unwrap();
    let same = a.read(NOTES).len();
    a.write(NOTES, &vec![b'y'; same]);
    std::fs::File::options().write(true).open(&p).unwrap().set_modified(old).unwrap();
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}

// Mutation: keep a cache entry for a file that was removed and written again with other bytes.
#[test]
fn a_removed_and_rewritten_file_is_read_again() {
    let w = big_instance(20);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "status", "research"]);
    let len = a.read(NOTES).len();
    a.remove(NOTES);
    a.write(NOTES, &vec![b'z'; len]);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}
