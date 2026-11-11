//! Every named migration, run through the real binary on consumers shaped like the ones v1 has, in
//! the order a consumer makes them: `round-filenames`, `directory-to-ref` for each directory, then
//! `references-to-letkajenkka`. Each dry run changes nothing, each real run loses no byte of any file
//! on disk (ignored ones included) and moves no reference anywhere but ahead, each run made again is
//! refused with nothing changed, and a clone made afterwards has every file the first one had.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::collections::BTreeSet;

use refs_e2e::consumer::{self, Shape};
use refs_e2e::whole::{self, Allowed, Entry};
use refs_e2e::{Clone, Out};

/// One migration as a consumer makes it: a dry run, the run, the same call again, each checked over
/// the whole tree and every reference, then the commit the run says to make. Returns the dry run.
fn step(c: &Clone, args: &[&str], allowed: &Allowed, commit: &str) -> Out {
    let before = whole::capture(c);
    let mut dry_args = args.to_vec();
    dry_args.push("--dry-run");
    let dry = c.ok(&dry_args);
    if let Err(e) = whole::kept(c, &before, &whole::capture(c), &Allowed::default()) {
        panic!("{args:?}: the dry run changed the tree:\n{e}");
    }
    let run = c.ok(args);
    let planned: Vec<&str> = dry.stdout.lines().filter(|l| !l.starts_with("dry run:")).collect();
    let made: Vec<&str> = run.stdout.lines().filter(|l| !l.starts_with("done:")).collect();
    assert_eq!(planned, made, "{args:?}: the dry run named other steps than the run made");
    let after = whole::capture(c);
    if let Err(e) = whole::kept(c, &before, &after, allowed) {
        panic!("{args:?}: the run lost something:\n{e}\n{}", run.all());
    }
    let again = c.run(args);
    assert!(!again.ok, "{args:?}: a migration made is refused: {}", again.all());
    if let Err(e) = whole::kept(c, &after, &whole::capture(c), &Allowed::default()) {
        panic!("{args:?}: the refusal changed the tree:\n{e}");
    }
    c.commit_all(commit);
    let again = c.run(args);
    assert!(!again.ok, "{args:?}: a migration made and committed is refused: {}", again.all());
    dry
}

fn bytes(p: &str) -> Vec<u8> {
    p.as_bytes().to_vec()
}

/// The whole migration of a consumer of `s` in a clone made from its v1 remote, every step checked.
/// Returns the world and the clone.
fn migrate(s: &Shape) -> (refs_e2e::World, std::path::PathBuf) {
    let w = consumer::remote(s);
    let root;
    {
        let c = w.clone_of("a");
        s.add_ignored_output(&c);
        // the engine sets the clone up on its first run, in its own build directory, before any migration
        let _ = c.run(&["status"]);
        let renamed = s.renames().into_iter().map(|(f, t)| (bytes(&f), bytes(&t))).collect();
        step(&c, &["migrate", "round-filenames"], &Allowed { renamed, ..Allowed::default() }, "chore: rename the dated rounds");
        for (ns, path, include) in s.moves() {
            let args = consumer::move_args(ns, path, include);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let config = Allowed { rewritten: [bytes("mockspace.toml")].into(), ..Allowed::default() };
            step(&c, &args, &config, &format!("chore: move `{ns}` to its ref"));
        }
        let want = s.rewritten();
        let dry = step(
            &c,
            &["migrate", "references-to-letkajenkka"],
            &Allowed { rewritten: want.clone(), ..Allowed::default() },
            "chore: rewrite references to the letkajenkka spelling",
        );
        let named: BTreeSet<Vec<u8>> = dry
            .stdout
            .lines()
            .filter_map(|l| l.strip_prefix("rewrite ")?.split(": ").next())
            .map(bytes)
            .collect();
        assert_eq!(named, want, "the files the reference migration rewrites:\n{}", dry.stdout);
        if s.refs {
            let o = c.run(&["data-refs", "commit"]);
            assert!(o.ok, "{}\n{}", o.all(), c.run(&["--lint-only", "--scope", "infra"]).all());
        }
        c.push();
        c.ok(&["data-refs", "verify"]);
        root = c.root.clone();
    }
    (w, root)
}

/// What a fresh clone of the migrated remote has once it is instantiated is what the clone that
/// migrated has, every file of it but the ignored ones, byte for byte and mode for mode.
fn a_fresh_clone_has_every_file(s: &Shape) {
    let (w, root) = migrate(s);
    let a = whole::disk(&root);
    let b = w.clone_of("b");
    if s.refs {
        let blocked = b.fails(&["--generate-only"]);
        assert!(blocked.all().contains("instantiate"), "{}", blocked.all());
        b.ok(&["data-refs", "instantiate"]);
        b.ok(&["data-refs", "verify"]);
    }
    let theirs = whole::disk(&b.root);
    // what the engine writes for itself beside the sources, untracked and in no ref, is no data
    let made = w.command("git", &root).args(["ls-files", "-z", "--others", "--exclude-standard"]).output().unwrap().stdout;
    let made: BTreeSet<&[u8]> = made.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
    let ignored = |p: &[u8]| {
        let shown = whole::shown(p);
        shown.contains("target/") || shown.ends_with("/.env") || made.contains(p)
    };
    let mut bad = Vec::new();
    for (p, e) in a.iter().filter(|(p, _)| !ignored(p)) {
        match theirs.get(p) {
            Some(t) if t == e => {},
            Some(t) => bad.push(format!("{}: {e:?} became {t:?}", whole::shown(p)).chars().take(300).collect()),
            None => bad.push(format!("{}: missing from the fresh clone", whole::shown(p))),
        }
    }
    for p in theirs.keys().filter(|p| !a.contains_key(*p) && !ignored(p)) {
        bad.push(format!("{}: only in the fresh clone", whole::shown(p)));
    }
    assert!(bad.is_empty(), "{}: {}", s.name, bad.join("\n"));
    // nothing a `.gitignore` keeps out, and no build output, reached a ref or the branch
    let refs = w.remote_git(&["for-each-ref", "--format=%(refname)"]);
    for r in refs.lines() {
        let listing = w.remote_git(&["ls-tree", "-r", "--name-only", r]);
        assert!(!listing.lines().any(|p| p.contains("target/") || p.ends_with(".env")), "{r}: {listing}");
    }
}

#[test]
fn a_consumer_with_no_data_refs_takes_the_other_migrations_and_loses_nothing() {
    a_fresh_clone_has_every_file(&consumer::PLAIN);
}

#[test]
fn a_consumer_with_a_large_mock_directory_migrates_and_loses_nothing() {
    a_fresh_clone_has_every_file(&consumer::LARGE);
}

#[test]
fn a_consumer_whose_mock_directory_is_not_called_mock_migrates_and_loses_nothing() {
    a_fresh_clone_has_every_file(&consumer::ELSEWHERE);
}

#[test]
fn a_consumer_with_links_executables_binaries_crlf_odd_names_and_a_filter_migrates_and_loses_nothing() {
    a_fresh_clone_has_every_file(&consumer::ODD);
}

// Mutation: write a rewrite with `\n` line ends; a template written with `\r\n` then changes on
// every line the rewrite did not touch.
#[test]
fn a_rewrite_keeps_the_line_ends_a_file_was_written_with() {
    let s = consumer::ODD;
    let (_w, root) = migrate(&s);
    for rel in ["crates/alpha/CRLF.md.tmpl", "registry/ruling/crlf.toml"] {
        let text = std::fs::read(root.join(s.at(rel))).unwrap();
        let lone = text.windows(2).filter(|w| w[1] == b'\n' && w[0] != b'\r').count();
        assert_eq!(lone, 0, "{rel}: {:?}", String::from_utf8_lossy(&text));
        assert!(String::from_utf8_lossy(&text).contains("seed::design::the_four_lanes"), "{rel} was rewritten");
    }
}

// Mutation: let the move read a filtered file from the index instead of the disk; the instance
// then holds the pointer where the person had the bytes. Whether a ref should hold the bytes or the
// pointer is op's call, asked in question ticket
// `a_data_ref_holds_a_filtered_file_as_its_full_bytes_not_the_p`.
#[test]
fn a_file_under_a_filter_is_held_as_the_bytes_the_person_has() {
    let s = consumer::ODD;
    let (w, root) = migrate(&s);
    let model = s.at("research/model.lfs");
    let disk = std::fs::read(root.join(&model)).unwrap();
    assert_eq!(disk, refs_e2e::fixture::bytes(7, 200_000), "the control: the clone had the bytes, not the pointer");
    let held = w.remote_git(&["show", "refs/heads/mock/research:model.lfs"]);
    let b = w.clone_of("b");
    b.ok(&["data-refs", "instantiate"]);
    assert_eq!(b.read(&model), disk, "a fresh clone has the bytes");
    // the ref holds what the instance had on disk: the bytes, not the pointer the branch held
    assert!(!held.starts_with("version lfs-like"), "the ref holds the pointer");
}

// Mutation: read the link the branch keeps as a file, or let a move take it; the fresh clone then
// has a file where the link was, or nothing.
#[test]
fn a_link_the_branch_keeps_beside_a_moved_directory_stays_a_link() {
    let s = consumer::ODD;
    let (w, _root) = migrate(&s);
    let b = w.clone_of("b");
    b.ok(&["data-refs", "instantiate"]);
    let link = b.path(&s.at("crates/alpha/src/again.rs"));
    assert_eq!(std::fs::read_link(&link).unwrap(), std::path::Path::new("lib.rs"));
    let entry = whole::disk(&b.root).remove(s.at("crates/alpha/src/again.rs").as_bytes()).unwrap();
    assert_eq!(entry, Entry::Link(b"lib.rs".to_vec()));
}

// Mutation: none; this pins what the phase gates make of the reference migration in a round whose
// doc changelist is locked. The templates held on a ref are rewritten in the instance, and the
// `data-refs commit` the run names as the next step is refused by the lints, which read the rewrite
// as a doc edit outside the DOC phase. Nothing is lost: the rewrite stays on disk as an edit and
// the ref keeps what it had. Whether the mechanical rewrite should pass the phase gates is op's
// call, asked in question ticket `the_references_rewrite_of_ref_held_templates_is_refused_by_a`;
// the day it does, this test says so.
#[test]
fn in_a_round_whose_doc_changelist_is_locked_the_rewritten_templates_wait_on_disk() {
    let s = consumer::ELSEWHERE;
    let w = consumer::remote(&s);
    let c = w.clone_of("a");
    let _ = c.run(&["status"]);
    c.remove(&s.at("design_rounds/2026-04-01_changelist.md"));
    c.write(&s.at("design_rounds/202611020000_changelist.doc.lock.md"), b"- the doc changelist, locked\n");
    c.commit_all("docs: lock the doc changelist");
    consumer::migrate_until_references(&s, &c);
    let readme = s.at("crates/alpha/README.md.tmpl");
    let tip = w.remote_tip("refs/heads/mock/templates").unwrap();
    let before = whole::capture(&c);
    c.ok(&["migrate", "references-to-letkajenkka"]);
    c.commit_all("chore: rewrite references to the letkajenkka spelling");
    let o = c.run(&["data-refs", "commit"]);
    assert!(!o.ok && o.all().contains("lints refuse"), "the rewrite on a ref passed the phase gates now: {}", o.all());
    let rewritten = s.rewritten();
    whole::kept(&c, &before, &whole::capture(&c), &Allowed { rewritten, ..Allowed::default() }).unwrap();
    assert!(String::from_utf8(c.read(&readme)).unwrap().contains("seed::design::the_four_lanes"), "the rewrite is on disk");
    assert_eq!(w.remote_tip("refs/heads/mock/templates").unwrap(), tip, "the ref is as it was");
    let s = c.ok(&["data-refs", "status", "templates"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}
