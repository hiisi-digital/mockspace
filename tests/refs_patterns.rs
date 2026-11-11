//! Include patterns changed after the fact, a file moved back to the branch, and two namespaces
//! in one repository.

mod refs_e2e;

use refs_e2e::{Held, check, fixture, held_on_remote, instance_matches, status_of, tip};

const GUIDE: &str = "mock/crates/alpha/docs/GUIDE.md.tmpl";
const TEMPLATES: Held = Held { ns: "templates", path: "crates" };

fn config(a: &refs_e2e::Clone) -> String {
    String::from_utf8(a.read("mock/mockspace.toml")).unwrap()
}

fn edit_config(a: &refs_e2e::Clone, from: &str, to: &str) {
    let text = config(a);
    assert!(text.contains(from), "the config has {from}: {text}");
    a.write("mock/mockspace.toml", text.replace(from, to).as_bytes());
}

// Mutation: let a narrower pattern read the tip's files outside it as removed, which deletes them
// from the instance.
#[test]
fn narrowing_the_patterns_refuses_and_deletes_nothing() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let before = tip(&w, "templates");
    edit_config(&a, "**/*.md.tmpl", "*/README.md.tmpl");
    for verb in ["status", "verify"] {
        let o = a.run(&["data-refs", verb, "templates"]);
        assert!(!o.ok && o.all().contains("GUIDE.md.tmpl"), "{verb}: {}", o.all());
    }
    assert!(a.exists(GUIDE) && a.exists("mock/crates/beta/docs/GUIDE.md.tmpl"));
    assert_eq!(tip(&w, "templates"), before);
    let held = held_on_remote(&w, &[TEMPLATES]);
    instance_matches(&a, &held, false).unwrap();
}

// Mutation: leave the branch's own Cargo.toml tracked when a pattern widens over it; two copies
// would then exist and the standing check below would not name them.
#[test]
fn widening_the_patterns_names_the_files_the_branch_still_tracks_and_the_way_through_keeps_them() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let manifests = ["mock/crates/alpha/Cargo.toml", "mock/crates/beta/Cargo.toml"];
    let before: Vec<Vec<u8>> = manifests.iter().map(|m| a.read(m)).collect();
    edit_config(&a, "**/*.md.tmpl", "**/*.md.tmpl\", \"**/Cargo.toml");
    let o = a.run(&["--generate-only"]);
    assert!(!o.ok, "{}", o.all());
    for m in manifests {
        assert!(o.all().contains(m), "the refusal names {m}: {}", o.all());
    }
    assert!(o.all().contains("git rm --cached"), "{}", o.all());
    // the stated remedy
    for m in manifests {
        a.git(&["rm", "-q", "--cached", m]);
    }
    a.git(&["add", "-f", "mock/mockspace.toml"]);
    a.commit_staged("chore: widen the templates ref");
    for (m, bytes) in manifests.iter().zip(&before) {
        assert_eq!(&a.read(m), bytes, "the files stay on disk");
    }
    a.ok(&["data-refs", "commit", "templates"]);
    let held = held_on_remote(&w, &[TEMPLATES]);
    for m in manifests {
        assert!(held.contains_key(m), "{m} is on the ref");
    }
    a.push();
    let b = fixture::seat(&w, "b");
    for (m, bytes) in manifests.iter().zip(&before) {
        assert_eq!(&b.read(m), bytes);
    }
    assert!(!b.tracked().iter().any(|p| manifests.contains(&p.as_str())));
}

// Mutation: let the standing check read a file the branch tracks as held by the tip only, so a
// pattern match alone is not noticed; the file would sit in both places.
#[test]
fn a_file_goes_from_its_ref_back_to_the_branch_by_deleting_it_there_and_narrowing() {
    // research held by patterns, so that a file can leave it
    let w2 = refs_e2e::World::new();
    let a = w2.first_clone("a");
    fixture::build_tracked(&a);
    let before = check::at(&a, "HEAD");
    fixture::migrate_one(&a, "research", "research", &["**/*.md", "**/*.txt"]);
    a.push();
    let crlf = "mock/research/crlf.txt";
    let bytes = a.read(crlf);
    a.remove(crlf);
    a.ok(&["data-refs", "commit", "research"]);
    assert!(!held_on_remote(&w2, &[Held { ns: "research", path: "research" }]).contains_key(crlf));
    edit_config(&a, "**/*.txt", "empty.txt");
    a.write(crlf, &bytes);
    a.git(&["add", "-f", crlf]);
    a.commit_all("chore: keep the crlf file on the branch");
    a.ok(&["data-refs", "verify"]);
    a.push();
    let b = w2.clone_of("b");
    b.ok(&["data-refs", "instantiate"]);
    assert_eq!(b.read(crlf), bytes, "the file arrives from the branch");
    let held = held_on_remote(&w2, &[Held { ns: "research", path: "research" }]);
    let branch = check::branch(&a);
    refs_e2e::no_loss(&before, &branch, &held, &["mock/mockspace.toml"]).unwrap();
}

// Mutation: make `commit` with no namespace write every ref, or `commit research` also the
// benches ref; the benches tip would then move.
#[test]
fn two_namespaces_are_committed_and_updated_independently() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    a.write("mock/research/notes with spaces.md", b"research edit\n");
    a.write("mock/benches/hot/baseline.json", b"{\"ns\": 99}\n");
    let benches = tip(&w, "benches");
    a.ok(&["data-refs", "commit", "research"]);
    assert_eq!(tip(&w, "benches"), benches, "the other ref is not written");
    assert!(status_of(&a, "benches").ends_with("changed"), "{}", status_of(&a, "benches"));
    assert!(status_of(&a, "research").ends_with("clean"), "{}", status_of(&a, "research"));
    b.ok(&["data-refs", "update", "research"]);
    assert_eq!(b.read("mock/research/notes with spaces.md"), b"research edit\n");
    assert_eq!(b.read("mock/benches/hot/baseline.json"), b"{\"ns\": 120}\n", "benches did not move");
    // without a namespace, every changed ref is committed
    a.ok(&["data-refs", "commit"]);
    assert_ne!(tip(&w, "benches"), benches);
    b.ok(&["data-refs", "update"]);
    assert_eq!(b.read("mock/benches/hot/baseline.json"), b"{\"ns\": 99}\n");
}
