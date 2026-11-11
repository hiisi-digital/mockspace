//! Landing with `trailers` and `paired`, a push race between two clones, a remote that cannot be
//! reached.

mod refs_e2e;

use refs_e2e::{Held, check, fixture, held_on_remote, instance_matches, tip};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const NAMESPACES: [&str; 4] = ["research", "benches", "rounds", "templates"];

fn land(c: &refs_e2e::Clone, trailers: &str) {
    let msg = format!("feat: land with the data\n\n{trailers}");
    c.git(&["commit", "-q", "--no-verify", "--allow-empty", "-m", &msg]);
}

// Mutation: print the instance's base instead of the tip it is at, or skip a declared ref; the
// lines would then not match the remote's tips.
#[test]
fn trailers_name_every_declared_ref_at_the_commit_its_instance_is_at() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let o = a.ok(&["data-refs", "trailers"]);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 4, "{}", o.stdout);
    for (line, ns) in lines.iter().zip(NAMESPACES) {
        let want = format!("Mock-Ref: {ns} {} ", tip(&w, ns));
        assert!(line.starts_with(&want), "{line} vs {want}");
        assert_eq!(line.split(' ').count(), 5, "hash, author time and digest follow: {line}");
    }
}

// Mutation: let `trailers` print for an instance with uncommitted changes; the landing would then
// claim data it does not hold.
#[test]
fn trailers_refuse_over_an_instance_with_changes_the_ref_does_not_hold() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(NOTES, b"not committed\n");
    let o = a.fails(&["data-refs", "trailers"]);
    assert!(o.stdout.is_empty(), "nothing is printed on a refusal: {}", o.stdout);
    assert!(o.all().contains("research"), "{}", o.all());
}

// Mutation: make `paired` accept a message with a missing line, or a hash no ref commit has.
#[test]
fn paired_passes_for_a_landing_with_every_line_and_fails_for_the_ones_that_miss() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let trailers = a.ok(&["data-refs", "trailers"]).stdout;
    land(&a, &trailers);
    a.ok(&["data-refs", "paired", "HEAD"]);
    // a line missing
    let fewer: String = trailers.lines().filter(|l| !l.contains(" benches ")).map(|l| format!("{l}\n")).collect();
    land(&a, &fewer);
    let o = a.fails(&["data-refs", "paired", "HEAD"]);
    assert!(o.all().contains("benches"), "{}", o.all());
    // a hash that is no commit of the ref, with a digest no commit of it gives
    let digest = trailers.lines().find(|l| l.contains(" research ")).unwrap().split(' ').nth(4).unwrap().to_string();
    let wrong = trailers.replace(&tip(&w, "research"), &"0".repeat(40)).replace(&digest, &"f".repeat(64));
    land(&a, &wrong);
    let o = a.fails(&["data-refs", "paired", "HEAD"]);
    assert!(o.all().contains("research"), "{}", o.all());
    // no lines at all
    land(&a, "");
    a.fails(&["data-refs", "paired", "HEAD"]);
}

// Mutation: drop the digest fallback, so that a ref made again with the same files is no longer
// found by the time and digest the trailer recorded.
#[test]
fn paired_finds_a_ref_made_again_with_the_same_files_by_its_time_and_digest() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let trailers = a.ok(&["data-refs", "trailers"]).stdout;
    land(&a, &trailers);
    a.push();
    // the research ref is made again: same tree, same author time, a new hash
    let old = tip(&w, "research");
    let line = trailers.lines().find(|l| l.contains(" research ")).unwrap();
    let time = line.split(' ').nth(3).unwrap().to_string();
    let tree = w.remote_git(&["rev-parse", &format!("{old}^{{tree}}")]).trim().to_string();
    let out = w
        .command("git", &w.remote)
        .args(["commit-tree", &tree, "-m", "mockspace data: research"])
        .env("GIT_AUTHOR_DATE", &time)
        .env("GIT_COMMITTER_DATE", &time)
        .output()
        .unwrap();
    assert!(out.status.success());
    let new = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert_ne!(new, old);
    w.remote_git(&["update-ref", "refs/heads/mock/research", &new]);
    // a clone that never saw the old hash reads it back by time and digest
    let fresh = fixture::seat(&w, "fresh");
    fresh.git(&["fetch", "-q", "origin", "dev"]);
    fresh.git(&["merge", "-q", "--ff-only", "origin/dev"]);
    fresh.ok(&["data-refs", "paired", "HEAD"]);
    // the control: the same new commit with a different tree is no pair
    let other = w.remote_git(&["rev-parse", &format!("{}^{{tree}}", tip(&w, "benches"))]).trim().to_string();
    let out = w
        .command("git", &w.remote)
        .args(["commit-tree", &other, "-m", "mockspace data: research"])
        .env("GIT_AUTHOR_DATE", &time)
        .env("GIT_COMMITTER_DATE", &time)
        .output()
        .unwrap();
    let bad = String::from_utf8(out.stdout).unwrap().trim().to_string();
    w.remote_git(&["update-ref", "refs/heads/mock/research", &bad]);
    a.fails(&["data-refs", "paired", "HEAD"]);
}

// Mutation: let the second push replace the winner's commit; b's file would then vanish from the
// ref, or a's edit would not be kept on disk after the refusal.
#[test]
fn a_push_race_refuses_the_late_clone_keeps_its_edit_and_lands_both_after_an_update() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    b.write("mock/research/from-b.md", b"b\n");
    a.write(NOTES, b"a's edit\n");
    b.ok(&["data-refs", "commit", "research"]);
    let winner = tip(&w, "research");
    let o = a.fails(&["data-refs", "commit", "research"]);
    assert!(o.all().contains("another writer"), "{}", o.all());
    assert_eq!(tip(&w, "research"), winner, "the winner's tip stands");
    assert_eq!(a.read(NOTES), b"a's edit\n", "the late clone keeps its edit");
    a.ok(&["data-refs", "update", "research"]);
    a.ok(&["data-refs", "commit", "research"]);
    let c = fixture::seat(&w, "c");
    assert_eq!(c.read(NOTES), b"a's edit\n");
    assert_eq!(c.read("mock/research/from-b.md"), b"b\n");
    instance_matches(&c, &held_on_remote(&w, &[Held { ns: "research", path: "research" }]), false).unwrap();
}

// Mutation: let `commit` take the late clone's file over the winner's when both changed one.
#[test]
fn a_race_on_one_file_loses_neither_edit() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.write(NOTES, b"a's edit\n");
    b.write(NOTES, b"b's edit\n");
    b.ok(&["data-refs", "commit", "research"]);
    a.fails(&["data-refs", "commit", "research"]);
    let o = a.run(&["data-refs", "update", "research"]);
    assert!(!o.ok, "a file changed on both sides is refused: {}", o.all());
    assert_eq!(a.read(NOTES), b"a's edit\n");
    let on_ref = w.remote_git(&["show", "refs/heads/mock/research:20261001_alpha/NOTES.md"]);
    assert_eq!(on_ref, "b's edit\n");
}

// Mutation: write the ref locally and report success when the push fails; the edit would then be
// believed landed. One call asks the remote, because each failed fetch retries ten times at two
// seconds, which is most of this test's time.
#[test]
fn an_unreachable_remote_refuses_commit_and_loses_nothing() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["remote", "set-url", "origin", "/nonexistent/remote.git"]);
    a.write(NOTES, b"offline edit\n");
    let t = tip(&w, "research");
    let o = a.fails(&["data-refs", "commit", "research"]);
    assert!(o.all().contains("fetch"), "{}", o.all());
    assert_eq!(a.read(NOTES), b"offline edit\n");
    assert_eq!(tip(&w, "research"), t);
    // `status` reads what the clone last saw and does not ask the remote
    assert!(a.ok(&["data-refs", "status", "research"]).stdout.contains("changed"));
    // and once the remote is back the edit goes through
    a.git(&["remote", "set-url", "origin", w.remote.to_str().unwrap()]);
    a.ok(&["data-refs", "commit", "research"]);
    let held = held_on_remote(&w, &[Held { ns: "research", path: "research" }]);
    assert_eq!(check::blob_id(&a, NOTES), held[NOTES].1);
}

// Mutation: none; the guide says `paired` reads a ref back after a rehouse, and the clone that
// landed the commit is the one that holds the old hash, so it is the case that matters.
#[test]
fn paired_survives_a_rehouse_in_the_clone_that_held_the_old_hash() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let trailers = a.ok(&["data-refs", "trailers"]).stdout;
    land(&a, &trailers);
    let old = tip(&w, "research");
    let line = trailers.lines().find(|l| l.contains(" research ")).unwrap();
    let time = line.split(' ').nth(3).unwrap().to_string();
    let tree = w.remote_git(&["rev-parse", &format!("{old}^{{tree}}")]).trim().to_string();
    let out = w
        .command("git", &w.remote)
        .args(["commit-tree", &tree, "-m", "mockspace data: research"])
        .env("GIT_AUTHOR_DATE", &time)
        .env("GIT_COMMITTER_DATE", &time)
        .output()
        .unwrap();
    let new = String::from_utf8(out.stdout).unwrap().trim().to_string();
    w.remote_git(&["update-ref", "refs/heads/mock/research", &new]);
    a.ok(&["data-refs", "paired", "HEAD"]);
}
