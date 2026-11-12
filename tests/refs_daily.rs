//! A consumer's days after the migration: two or three clones of one remote, edits on both sides of
//! the data refs and of the branch, and the git commands people run. Every scenario writes through
//! a ledger, and ends asking that every text written is still on a disk or in a commit somewhere a
//! person gets it back from, and that no file a ref holds was committed to the branch.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::daily::{NOTES, ROUND, SRC, TMPL, migrated, nothing_lost_or_wrongly_committed, seat};
use refs_e2e::whole::Ledger;

// Mutation: let `update` write the tip over a file that holds an edit, or let `commit` take a file the
// branch tracks; an edit on one side of the ref is then lost on the other, or reaches the branch.
#[test]
fn edits_on_both_sides_of_the_ref_and_the_branch_meet_through_pull_and_update() {
    let (w, base) = migrated();
    let (a, b) = (seat(&w, "a"), seat(&w, "b"));
    let mut l = Ledger::default();
    l.write(&a, NOTES, b"a: notes\n");
    l.write(&a, "mock/research/new-by-a.md", b"a: a new file\n");
    l.write(&b, SRC, b"pub fn alpha() { let b = 1; }\n");
    l.write(&b, TMPL, b"b: the template\n");
    b.commit_all("feat: b's source");
    b.push();
    b.ok(&["data-refs", "commit", "templates"]);
    a.ok(&["data-refs", "commit", "research"]);
    a.git(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    a.ok(&["data-refs", "update"]);
    b.ok(&["data-refs", "update"]);
    assert_eq!(a.read(TMPL), b"b: the template\n");
    assert_eq!(b.read(NOTES), b"a: notes\n");
    assert_eq!(b.read("mock/research/new-by-a.md"), b"a: a new file\n");
    // a commit of a's own on the branch, an edit of the instance left as it is, and `pull --rebase`
    l.write(&a, "mock/crates/beta/src/lib.rs", b"pub fn beta() { let a = 2; }\n");
    a.commit_all("feat: a's source");
    l.write(&a, NOTES, b"a: second, not committed\n");
    l.write(&b, "README.md", b"# orchard, by b\n");
    b.commit_all("docs: b's readme");
    b.push();
    a.git(&["pull", "-q", "--rebase", "origin", "dev"]);
    a.push();
    assert_eq!(a.read(NOTES), b"a: second, not committed\n", "the rebase left the instance as it was");
    a.ok(&["data-refs", "commit", "research"]);
    b.git(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "dev"]);
    b.ok(&["data-refs", "update"]);
    assert_eq!(b.read(NOTES), b"a: second, not committed\n");
    a.ok(&["data-refs", "verify"]);
    b.ok(&["data-refs", "verify"]);
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a, &b]);
    // a clone made now has what both made
    let c = seat(&w, "c");
    for p in [NOTES, TMPL, SRC, "mock/research/new-by-a.md", "README.md"] {
        assert_eq!(c.read(p), a.read(p), "{p}");
    }
}

// Mutation: let `update` write the tip over a file edited here, or `commit` push over a tip it was
// not built on; one person's text is then nowhere. This also pins that the tools give no way through
// a file changed on both sides: `commit` names `update`, which refuses the same file, and the person
// sets the edit aside by hand. How that conflict is resolved is op's call, asked in question ticket
// `a_ref_file_changed_in_two_clones_has_no_engine_way_through`.
#[test]
fn one_ref_file_edited_by_two_people_keeps_both_edits_and_is_resolved_by_hand() {
    let (w, base) = migrated();
    let (a, b) = (seat(&w, "a"), seat(&w, "b"));
    let mut l = Ledger::default();
    let rel = ROUND.strip_prefix("mock/design_rounds/").unwrap();
    l.write(&a, ROUND, b"a: the round\n");
    l.write(&b, ROUND, b"b: the round\n");
    a.ok(&["data-refs", "commit", "rounds"]);
    let tip = w.remote_tip("refs/heads/mock/rounds");
    let c = b.fails(&["data-refs", "commit", "rounds"]);
    assert!(c.all().contains("data-refs update"), "the refusal names the way on: {}", c.all());
    let u = b.fails(&["data-refs", "update", "rounds"]);
    assert!(u.all().contains(rel), "the refusal names the file: {}", u.all());
    assert_eq!(b.read(ROUND), b"b: the round\n");
    assert_eq!(w.remote_tip("refs/heads/mock/rounds"), tip, "a refused commit pushed nothing");
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a, &b]);
    // by hand: the edit set aside, the file put back as the base has it (a removal is a change here
    // too, which `update` refuses as well), the tip taken, the text both agree on written and committed
    let mine = b.read(ROUND);
    b.remove(ROUND);
    let gone = b.fails(&["data-refs", "update", "rounds"]);
    assert!(gone.all().contains(rel), "{}", gone.all());
    b.ok(&["data-refs", "restore", "rounds"]);
    b.ok(&["data-refs", "update", "rounds"]);
    assert_eq!(b.read(ROUND), b"a: the round\n");
    let both = [b.read(ROUND), mine].concat();
    l.write(&b, ROUND, &both);
    b.ok(&["data-refs", "commit", "rounds"]);
    a.ok(&["data-refs", "update", "rounds"]);
    assert_eq!(a.read(ROUND), both);
    nothing_lost_or_wrongly_committed(&w, &l, &base, &[&a, &b]);
}

