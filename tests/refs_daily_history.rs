//! A consumer's days after the migration that move through history: a cherry-pick, a hard reset to a
//! commit from before the migration and back, a checkout of one, a linked worktree, a stash, and a
//! clone made before the migration that pulls it. Every scenario writes through a ledger and ends
//! asking that every text written is on a disk or in a commit somewhere a person gets it back from.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::consumer;
use refs_e2e::daily::{NOTES, S, SRC, TMPL, before_the_migration, migrated, nothing_lost_or_wrongly_committed, seat};
use refs_e2e::whole::Ledger;

// Mutation: drop the copy the hook takes when a reset moves `ORIG_HEAD`; a reset to a commit from
// before the migration and back then removes the edit with every file that commit tracked.
#[test]
fn a_cherry_pick_and_a_hard_reset_across_the_migration_and_back_lose_no_edit() {
    let (w, base) = migrated();
    let a = seat(&w, "a");
    let mut l = Ledger::default();
    a.git(&["checkout", "-q", "-b", "side"]);
    l.write(&a, "mock/crates/beta/src/lib.rs", b"pub fn beta() { let side = 3; }\n");
    a.commit_all("feat: a source on the side");
    a.git(&["checkout", "-q", "dev"]);
    l.write(&a, NOTES, b"a: edited before the cherry-pick\n");
    l.write(&a, TMPL, b"a: the template edited before the cherry-pick\n");
    a.git(&["cherry-pick", "side"]);
    assert_eq!(a.read(NOTES), b"a: edited before the cherry-pick\n");
    let here = a.git(&["rev-parse", "HEAD"]).trim().to_string();
    let pre = before_the_migration(&a);
    a.git(&["reset", "-q", "--hard", &pre]);
    a.git(&["reset", "-q", "--hard", &here]);
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a]);
    let s = a.run(&["data-refs", "status"]);
    a.ok(&["data-refs", "restore"]);
    for (p, text) in [(NOTES, &b"a: edited before the cherry-pick\n"[..]), (TMPL, b"a: the template edited before the cherry-pick\n")] {
        if a.read(p) != text {
            let (ns, lead) = if p == NOTES { ("research", "mock/research/") } else { ("templates", "mock/crates/") };
            a.ok(&["data-refs", "restore", ns, p.strip_prefix(lead).unwrap()]);
        }
        assert_eq!(a.read(p), text, "{p}: {}", s.all());
    }
    a.ok(&["data-refs", "commit"]);
    a.ok(&["data-refs", "verify"]);
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a]);
}

// Mutation: write the sparse block only on `instantiate` and never on the migration's own clone; a
// checkout of a commit from before the migration then writes over the edit.
//
// Then a pin: back from there, git refuses a plain checkout even in a clone nobody edited, since the
// reference migration rewrote templates the old commit tracks, so they read as local changes there.
// Nothing is lost by that refusal; what the forced ways back do is pinned below.
#[test]
fn a_checkout_of_a_commit_from_before_the_migration_keeps_the_edit_and_the_way_back_is_refused() {
    let (w, base) = migrated();
    let a = seat(&w, "a");
    let mut l = Ledger::default();
    l.write(&a, NOTES, b"a: edited before an old checkout\n");
    let pre = before_the_migration(&a);
    a.git(&["checkout", "-q", &pre]);
    assert_eq!(a.read(NOTES), b"a: edited before an old checkout\n");
    let back = a.git_out(&["checkout", "-q", "dev"]);
    assert!(!back.ok && back.all().contains("overwritten"), "{}", back.all());
    assert_eq!(a.read(NOTES), b"a: edited before an old checkout\n");
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a]);
    let b = seat(&w, "b");
    b.git(&["checkout", "-q", &pre]);
    let back = b.git_out(&["checkout", "-q", "dev"]);
    assert!(!back.ok, "a clean clone goes back now; drop this pin: {}", back.all());
    assert!(back.all().contains("README.md.tmpl"), "{}", back.all());
}

/// Whether any copy in the history of the backup ref of `ns` holds `text`.
fn history_holds(c: &refs_e2e::Clone, ns: &str, text: &[u8]) -> bool {
    history_holds_at(c, ns, "NOTES.md", text)
}

/// Whether any copy in the history of the backup ref of `ns` holds `text` at a path ending in `name`.
fn history_holds_at(c: &refs_e2e::Clone, ns: &str, name: &str, text: &[u8]) -> bool {
    let commits = c.git_out(&["rev-list", &format!("refs/mock-backup/{ns}")]);
    commits.stdout.lines().any(|commit| {
        let tree = c.git(&["ls-tree", "-r", "-z", "--name-only", commit]);
        tree.split('\0').filter(|p| p.ends_with(name)).any(|p| c.git(&["show", &format!("{commit}:{p}")]).as_bytes() == text)
    })
}

// Mutation: none; this pins a loss the design does not name. An edit made on `dev` and carried to a
// commit from before the migration, which the sparse checkout keeps, is a tracked file there, and the
// forced ways back (`checkout -f`, `reset --hard`) drop it. No engine command saw it, and one run at
// the old commit declares no ref, so no copy holds it. Where an engine command saw it on `dev`, the
// copy of edits keeps it, but only in the history of the backup ref: nothing names it after the
// forced way back, and `restore` writes the base's bytes. Which of the hook, the engine at an old
// commit or the copy closes it is op's call, asked in question ticket
// `the_way_back_from_a_pre_migration_commit_drops_an_edit_carri`; the day one does, this test says so.
#[test]
fn the_forced_way_back_from_a_commit_from_before_the_migration_drops_an_edit_carried_there() {
    for (way, looked) in [(&["checkout", "-q", "-f", "dev"][..], false), (&["reset", "-q", "--hard", "dev"], false), (&["checkout", "-q", "-f", "dev"], true)] {
        let (w, _) = migrated();
        let a = seat(&w, "a");
        let mut l = Ledger::default();
        let text = b"a: edited, then an old checkout\n";
        l.write(&a, NOTES, text);
        if looked {
            let _ = a.run(&["data-refs", "status"]);
        }
        let pre = before_the_migration(&a);
        a.git(&["checkout", "-q", &pre]);
        let at_old = a.run(&["data-refs", "status"]);
        assert!(at_old.all().contains("no [[data_ref]] is declared"), "{}", at_old.all());
        a.git(way);
        let _ = a.git_out(&["checkout", "-q", "dev"]);
        let s = a.run(&["data-refs", "status", "research"]);
        a.ok(&["data-refs", "restore", "research"]);
        assert_ne!(a.read(NOTES), text, "{way:?}: the edit is back on disk; drop this pin");
        let after = a.run(&["data-refs", "status", "research"]);
        assert!(!after.all().contains("written over"), "{way:?}: the edit is said now; drop this pin: {}", after.all());
        if looked {
            assert!(history_holds(&a, "research", text), "{way:?}: the copy of edits held it: {}", s.all());
            l.kept(&w, &[&a]).unwrap();
        } else {
            assert!(l.kept(&w, &[&a]).is_err(), "{way:?}: the edit is somewhere now; drop this pin");
        }
    }
}

// Mutation: keep the instance of a linked worktree in the main worktree's record; a commit from one
// then reads the other's files as its own.
#[test]
fn a_linked_worktree_edits_and_commits_beside_the_main_one() {
    let (w, base) = migrated();
    let a = seat(&w, "a");
    let mut l = Ledger::default();
    let wt = a.at(a.worktree_add("wt", "work/wt"));
    wt.ok(&["data-refs", "instantiate"]);
    l.write(&wt, NOTES, b"wt: notes\n");
    l.write(&wt, "mock/crates/beta/src/lib.rs", b"pub fn beta() { let wt = 4; }\n");
    l.write(&a, TMPL, b"a: the template\n");
    wt.commit_all("feat: the worktree's source");
    wt.ok(&["data-refs", "commit", "research"]);
    a.ok(&["data-refs", "commit", "templates"]);
    a.ok(&["data-refs", "update", "research"]);
    wt.ok(&["data-refs", "update", "templates"]);
    assert_eq!(a.read(NOTES), b"wt: notes\n");
    assert_eq!(wt.read(TMPL), b"a: the template\n");
    a.ok(&["data-refs", "verify"]);
    wt.ok(&["data-refs", "verify"]);
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a, &wt]);
}

// Mutation: read an instance none of whose files are on disk as one whose files were removed; a
// `commit` after a stash of everything then pushes the removal of every file of the ref.
//
// And a pin: an engine run while the stash holds everything makes its build directory again, and
// `stash pop` then says it could not restore the untracked files and keeps the stash, having written
// back every other file; the person drops the stash once they see their files.
#[test]
fn a_stash_of_everything_takes_the_instances_and_its_pop_gives_them_back() {
    let (w, base) = migrated();
    let a = seat(&w, "a");
    let mut l = Ledger::default();
    l.write(&a, NOTES, b"a: before a stash\n");
    l.write(&a, SRC, b"pub fn alpha() { let stashed = 5; }\n");
    // a stash of tracked and untracked files leaves the instances, which are neither
    a.git(&["stash", "push", "-q", "--include-untracked"]);
    assert_eq!(a.read(NOTES), b"a: before a stash\n");
    a.git(&["stash", "pop", "-q"]);
    let tips = || -> Vec<Option<String>> { ["research", "templates"].iter().map(|ns| w.remote_tip(&format!("refs/heads/mock/{ns}"))).collect() };
    let before = tips();
    a.git(&["stash", "push", "-q", "--all"]);
    assert!(!a.exists(NOTES), "the control: a stash of everything takes the ignored files too");
    let c = a.run(&["data-refs", "commit"]);
    assert!(!c.ok, "a commit over instances the stash took: {}", c.all());
    assert_eq!(tips(), before, "nothing was pushed");
    let pop = a.git_out(&["stash", "pop", "-q"]);
    assert!(!pop.ok && pop.all().contains("mock/target/"), "the pop goes through now; drop this pin: {}", pop.all());
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a]);
    assert!(a.git_out(&["rev-parse", "-q", "--verify", "refs/stash"]).ok, "the stash is kept");
    assert_eq!(a.read(NOTES), b"a: before a stash\n");
    assert_eq!(a.read(SRC), b"pub fn alpha() { let stashed = 5; }\n");
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a]);
}

// Mutation: let `instantiate` write the ref's file over one that is there, so the edit a clone made
// before the migration reached it is replaced by the ref's bytes; or refuse a directory that holds
// only build output and ignored files, which a person then deletes to get past, secrets included.
#[test]
fn a_clone_made_before_the_migration_pulls_it_with_an_edit_and_loses_nothing() {
    let w = consumer::remote(&S);
    let old = w.clone_of("old");
    S.add_ignored_output(&old);
    let _ = old.run(&["status"]);
    let mut l = Ledger::default();
    l.write(&old, NOTES, b"old: not committed\n");
    l.write(&old, TMPL, b"old: the template, not committed\n");
    consumer::migrate_all(&S, &w.clone_of("migrator"));
    // git refuses the pull over the two edits, as for any file the other side deleted
    let p = old.git_out(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    assert!(!p.ok && p.all().contains("overwritten"), "{}", p.all());
    assert_eq!(old.read(NOTES), b"old: not committed\n");
    assert_eq!(old.read(TMPL), b"old: the template, not committed\n");
    l.kept(&w, &[&old]).unwrap();
    // the way git names: commit, pull, and the edit against the deletion is a conflict, resolved as the
    // migration did, the file off the branch and left on disk
    old.commit_all("docs: edits made before the migration reached this clone");
    let m = old.git_out(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    assert!(!m.ok, "{}", m.all());
    old.git(&["rm", "-q", "--cached", "--sparse", NOTES, TMPL]);
    old.commit_staged("merge: take the migration");
    // the two edits set aside, every ref instantiated, the edits put back and committed to their refs
    let (notes, tmpl) = (old.read(NOTES), old.read(TMPL));
    old.remove(NOTES);
    old.remove(TMPL);
    let env = old.read("mock/research/.env");
    // what the directories still hold is build output and a secret a `.gitignore` keeps out, which an
    // instance never reads, so it is no reason to refuse
    old.ok(&["data-refs", "instantiate"]);
    assert_eq!(old.read("mock/research/.env"), env, "the ignored secret is left as it was");
    assert!(old.exists("mock/benches/hot/target/debug/out.o"), "the build output is left as it was");
    l.write(&old, NOTES, &notes);
    l.write(&old, TMPL, &tmpl);
    old.ok(&["data-refs", "commit"]);
    old.push();
    old.ok(&["data-refs", "verify"]);
    let fresh = seat(&w, "fresh");
    assert_eq!(fresh.read(NOTES), notes);
    assert_eq!(fresh.read(TMPL), tmpl);
    l.kept(&w, &[&old, &fresh]).unwrap();
}

/// A merge of a branch cut before the migration that writes over an edit no engine command saw,
/// while the backup ref is locked, as the reftable backend always has it locked inside the hook: the
/// copy is made as objects and named only by a file beside the instance's record.
fn a_copy_named_by_a_file_only(w: &refs_e2e::World) -> refs_e2e::Clone<'_> {
    let a = refs_e2e::fixture::dirty_instance_and_an_old_branch(w);
    let common = a.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let lock = std::path::Path::new(common.trim()).join("refs/mock-backup/research.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, b"").unwrap();
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    std::fs::remove_file(&lock).unwrap();
    assert!(!a.git_out(&["rev-parse", "-q", "--verify", "refs/mock-backup/research"]).ok, "the control: the ref took the copy");
    a
}

const DIRTY: &[u8] = b"DIRTY instance edit\n";
const IN_REF: &str = "20261001_alpha/NOTES.md";

// Mutation: let `gc` prune the objects of a copy named by a file only; the edit git wrote over is
// then in no copy. A copy in the backup ref is reachable and kept by any `gc`.
#[test]
fn gc_keeps_every_copy_of_an_edit_git_wrote_over() {
    let w = refs_e2e::World::new();
    let a = a_copy_named_by_a_file_only(&w);
    a.git(&["gc", "-q"]);
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(refs_e2e::fixture::NOTES), DIRTY);
    // the next look took the copy into the backup ref, which a prune of everything unreachable keeps
    a.write(refs_e2e::fixture::NOTES, b"edited once more\n");
    let _ = a.run(&["data-refs", "status"]);
    a.git(&["gc", "-q", "--prune=now"]);
    assert!(history_holds(&a, "research", DIRTY) && history_holds(&a, "research", b"edited once more\n"));
}

// Mutation: none; this pins the window the design names: until the next engine command the objects
// of a copy named by a file only are named by nothing git reads, and a `gc` that prunes at once
// (`--prune=now`, or a `gc.pruneExpire` of now) takes them. The edit git wrote over is then in no
// copy, and the next look fails rather than say the file is safe.
//
// Mutation for the rest: let `commit --accept-overwritten` forget the copy that cannot be read and
// commit the bytes on disk; the design has a copy that cannot be read fail `commit`, since what git
// wrote over is then not known. Whether there is a way past it is op's call, asked in question
// ticket `the_way_past_a_copy_a_git_gc_pruned_is_provisional`.
#[test]
fn gc_that_prunes_at_once_takes_a_copy_no_engine_command_has_taken_yet() {
    let w = refs_e2e::World::new();
    let a = a_copy_named_by_a_file_only(&w);
    let mut l = Ledger::default();
    l.write(&a, "mock/research/after.md", b"written after the merge\n");
    a.git(&["gc", "-q", "--prune=now"]);
    let s = a.run(&["data-refs", "status", "research"]);
    let r = a.run(&["data-refs", "restore", "research", IN_REF]);
    assert!(!r.ok, "the copy outlived the prune; drop this pin: {}", r.all());
    let notes = refs_e2e::fixture::NOTES;
    assert!(!a.exists(notes) || a.read(notes) != DIRTY);
    let all = format!("{}\n{}", s.all(), r.all());
    let gone = [DIRTY].iter().all(|t| !history_holds(&a, "research", t));
    assert!(gone, "the edit is in the backup ref; drop this pin: {all}");
    l.kept(&w, &[&a]).unwrap();
    // the lost copy is said, and `commit` fails with or without `--accept-overwritten`, naming the
    // file that names the copy, and changes nothing
    assert!(all.contains("no longer holds"), "{all}");
    let tip = a.git_out(&["rev-parse", "-q", "--verify", "refs/heads/mock/research"]).stdout;
    for args in [&["data-refs", "commit", "research"][..], &["data-refs", "commit", "research", "--accept-overwritten"]] {
        let before = refs_e2e::whole::capture(&a);
        let c = a.fails(args);
        assert!(c.all().contains("no longer holds") && c.all().contains(".guard"), "{args:?} names the lost copy: {}", c.all());
        refs_e2e::whole::kept(&a, &before, &refs_e2e::whole::capture(&a), &refs_e2e::whole::Allowed::default()).unwrap();
    }
    assert_eq!(a.git_out(&["rev-parse", "-q", "--verify", "refs/heads/mock/research"]).stdout, tip, "nothing was committed");
    l.kept(&w, &[&a]).unwrap();
}
