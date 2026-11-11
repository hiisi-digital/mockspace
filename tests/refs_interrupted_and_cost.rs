//! What an instance command does when git fails part way, and what a gated run costs in proportion
//! to the bytes of the instance. A `git` in front of the real one on `PATH` logs each call and fails
//! the subcommands a test names, so the failure is real and happens at one chosen step.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::path::PathBuf;
use std::time::Instant;

use refs_e2e::{World, fixture};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";

/// Puts a logging `git` first on the `PATH` of every command the world runs. Returns the log and
/// the file whose lines name the subcommands to fail.
fn counting_git(w: &World) -> (PathBuf, PathBuf) {
    let base = w.base();
    let (log, fail) = (base.join("git.log"), base.join("git.fail"));
    let real = w.sh(&base, "sh", &["-c", "command -v git"]).stdout.trim().to_string();
    let body = format!(
        "#!/bin/sh\nsub=; skip=0\nfor a in \"$@\"; do\n  if [ $skip = 1 ]; then skip=0; continue; fi\n  case \"$a\" in -C|-c) skip=1;; --*) ;; *) sub=$a; break;; esac\ndone\necho \"$sub $*\" >> '{log}'\nif [ -f '{fail}' ] && grep -qx \"$sub\" '{fail}'; then echo \"injected failure of git $sub\" >&2; exit 1; fi\nexec '{real}' \"$@\"\n",
        log = log.display(),
        fail = fail.display(),
        real = real
    );
    mockspace_manifest::write_executable(&base.join("shims/git"), body).unwrap();
    (log, fail)
}

fn calls(log: &PathBuf, what: &str) -> usize {
    std::fs::read_to_string(log).unwrap_or_default().lines().filter(|l| l.contains(what)).count()
}

fn remote_notes(w: &World) -> String {
    w.remote_git(&["show", "refs/heads/mock/research:20261001_alpha/NOTES.md"])
}

// Mutation: write the edit's commit to the remote ahead of finishing locally, or clear the edit
// before the commit is made; a failure then loses the edit or leaves two versions of it.
#[test]
fn a_commit_that_fails_at_any_step_keeps_the_edit_and_the_retry_commits_it_once() {
    for step in ["write-tree", "commit-tree", "update-ref", "push", "fetch"] {
        let w = fixture::migrated_remote();
        let (log, fail) = counting_git(&w);
        let a = fixture::seat(&w, "a");
        a.write(NOTES, b"an edit that must survive\n");
        a.write("mock/research/new.md", b"a new file\n");
        let before = w.remote_tip("refs/heads/mock/research").unwrap();
        std::fs::write(&fail, format!("{step}\n")).unwrap();
        let hit = calls(&log, &format!("{step} "));
        let o = a.run(&["data-refs", "commit", "research"]);
        std::fs::remove_file(&fail).unwrap();
        let reached = calls(&log, &format!("{step} ")) > hit;
        assert_eq!(a.read(NOTES), b"an edit that must survive\n", "{step}: the edit is on disk");
        assert_eq!(a.read("mock/research/new.md"), b"a new file\n", "{step}: the new file is on disk");
        if reached {
            assert!(!o.ok, "{step}: the command was told to fail and said it succeeded: {}", o.all());
        }
        // where the push landed and the local ref was not moved, the commit is on the remote and the
        // instance is behind it: the way on is named, and `update` takes it
        let again = a.run(&["data-refs", "commit", "research"]);
        if !again.ok {
            assert!(again.all().contains("data-refs update"), "{step}: a refusal with no way on: {}", again.all());
            a.ok(&["data-refs", "update", "research"]);
            a.ok(&["data-refs", "commit", "research"]);
        }
        assert_eq!(a.read(NOTES), b"an edit that must survive\n", "{step}: after the recovery");
        assert_eq!(remote_notes(&w), "an edit that must survive\n", "{step}");
        let n = w.remote_git(&["rev-list", "--count", "refs/heads/mock/research"]);
        let was = w.remote_git(&["rev-list", "--count", &before]);
        assert_eq!(n.trim().parse::<u32>().unwrap(), was.trim().parse::<u32>().unwrap() + 1, "{step}: committed once");
    }
}

// Mutation: remove the files the tip removed and fail to write the rest with nothing recorded and the
// next run not taking up where it stopped.
#[test]
fn an_update_that_fails_while_reading_blobs_loses_no_local_edit_and_the_retry_converges() {
    let w = fixture::migrated_remote();
    let (_log, fail) = counting_git(&w);
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    b.write(NOTES, b"b changed the notes\n");
    b.write("mock/research/from-b.md", b"new from b\n");
    b.remove("mock/research/empty.txt");
    b.ok(&["data-refs", "commit", "research"]);
    a.write("mock/research/crlf.txt", b"a's own edit\n");
    std::fs::write(&fail, "cat-file\n").unwrap();
    let o = a.run(&["data-refs", "update", "research"]);
    std::fs::remove_file(&fail).unwrap();
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

fn big_instance(files: usize) -> (World, String) {
    let w = World::new();
    let a = w.first_clone("seed");
    fixture::build_tracked(&a);
    for i in 0..files {
        a.write(&format!("mock/research/bulk/{:03}/f{i:05}.md", i % 200), format!("file number {i}\n").repeat(8).as_bytes());
    }
    a.commit_all("docs: bulk research");
    fixture::migrate_one(&a, "research", "research", &[]);
    a.push();
    (w, "seed".into())
}

// Mutation: hash every file of the instance on every run, as the first version did: the second
// run of an unchanged instance then makes as many `hash-object` calls as the first.
#[test]
fn an_unchanged_instance_is_not_hashed_again_on_the_next_run() {
    let (w, _) = big_instance(1500);
    let (log, _fail) = counting_git(&w);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    let t0 = Instant::now();
    a.ok(&["data-refs", "status", "research"]);
    let first = t0.elapsed();
    let hashed_first = calls(&log, "hash-object --no-filters --stdin-paths");
    std::fs::write(&log, "").unwrap();
    let t1 = Instant::now();
    let s = a.ok(&["data-refs", "status", "research"]);
    let second = t1.elapsed();
    eprintln!("timing, test profile, 1500 files: first status {first:?}, second status {second:?}");
    assert!(s.stdout.trim_end().ends_with("clean"), "{}", s.all());
    assert!(hashed_first > 0 || calls(&log, "hash-object") == 0, "the control: the first run hashes");
    assert_eq!(calls(&log, "hash-object --no-filters --stdin-paths"), 0, "the second run hashed files again");
}

// Mutation: trust the cache on the size alone; an edit of the same length is then never seen.
#[test]
fn an_edit_of_the_same_size_straight_after_a_run_is_still_seen() {
    let (w, _) = big_instance(20);
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
    let (w, _) = big_instance(20);
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
    let (w, _) = big_instance(20);
    let a = w.clone_of("a");
    a.ok(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "status", "research"]);
    let len = a.read(NOTES).len();
    a.remove(NOTES);
    a.write(NOTES, &vec![b'z'; len]);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}
