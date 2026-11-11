//! A ref honours the branch's `.gitignore`: a file one matches is never committed and never removed
//! by the engine, `status` names it, and the migration names the same files in its dry run and its
//! real run. A file the ref already holds stays held, as a tracked file does in git.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{World, fixture};

fn on_remote(w: &World) -> String {
    w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"])
}

fn ignore(a: &refs_e2e::Clone, lines: &str) {
    let mut text = String::from_utf8(a.read(".gitignore")).unwrap();
    text.push_str(lines);
    a.write(".gitignore", text.as_bytes());
    a.commit_all("chore: ignore more");
}

// Mutation: read every file of the instance as data, as the version that dropped the ignore rule
// did; a secret the branch's `.gitignore` keeps out of every commit then reaches the ref and is
// pushed.
#[test]
fn a_dot_env_in_a_held_directory_never_reaches_the_ref() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    ignore(&a, ".env\nsecrets/\n");
    a.write("mock/research/.env", b"TOKEN=do-not-push\n");
    a.write("mock/research/20261001_alpha/.env", b"TOKEN=nor-this\n");
    a.write("mock/research/secrets/key.pem", b"a key\n");
    a.write("mock/research/kept.md", b"data\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    for named in [".env", "20261001_alpha/.env", "secrets/"] {
        assert!(s.stdout.contains(&format!("research: ignored, not held: {named}\n")), "{named} is not named: {}", s.all());
    }
    assert!(s.stdout.trim_end().ends_with("changed"), "the one file that is data is an edit: {}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let held = on_remote(&w);
    assert!(held.contains("kept.md"), "the control: the commit carried the data: {held}");
    assert!(!held.contains(".env") && !held.contains("secrets/"), "an ignored file reached the ref: {held}");
    // never removed by the engine either
    a.ok(&["data-refs", "update", "research"]);
    assert_eq!(a.read("mock/research/.env"), b"TOKEN=do-not-push\n");
    assert_eq!(a.read("mock/research/secrets/key.pem"), b"a key\n");
    a.ok(&["data-refs", "verify", "research"]);
}

// Mutation: drop a file the ref holds the moment a `.gitignore` matches it; the next commit then
// removes it from the ref, which git never does to a tracked file.
#[test]
fn a_file_the_ref_holds_stays_held_when_a_gitignore_matches_it_later() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    ignore(&a, "*.bin\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "a held file read as removed: {}", s.all());
    assert!(!s.stdout.contains("data.bin"), "a held file named as ignored: {}", s.all());
    a.write("mock/research/20261001_alpha/data.bin", b"changed bytes\n");
    a.write("mock/research/new.bin", b"a new one is ignored\n");
    a.ok(&["data-refs", "commit", "research"]);
    let held = on_remote(&w);
    assert!(held.contains("20261001_alpha/data.bin") && held.contains("large.bin"), "{held}");
    assert!(!held.contains("new.bin"), "{held}");
    assert_eq!(w.remote_git(&["show", "refs/heads/mock/research:20261001_alpha/data.bin"]), "changed bytes\n");
}

// Mutation: name the ignored files in one run of the migration and not the other, or move them.
#[test]
fn the_migration_names_the_ignored_files_in_its_dry_run_and_its_real_run() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    ignore(&a, ".env\n");
    a.write("mock/research/.env", b"TOKEN=x\n");
    let mut args = fixture::migrate_args("research", "research", &[]);
    args.push("--dry-run".into());
    let dry: Vec<&str> = args.iter().map(String::as_str).collect();
    let d = a.ok(&dry);
    let r = a.ok(&dry[.. dry.len() - 1]);
    let named = |o: &refs_e2e::Out| -> Vec<String> {
        o.stdout.lines().filter(|l| l.contains("ignored")).map(str::to_owned).collect()
    };
    assert_eq!(named(&d), vec!["leave mock/research/.env: ignored, not held".to_string()], "{}", d.all());
    assert_eq!(named(&d), named(&r), "the dry run and the real run differ: {}\n{}", d.all(), r.all());
    a.git(&["add", "-f", "--", "mock/mockspace.toml"]);
    a.commit_all("chore: move `research` to its ref");
    assert!(!on_remote(&w).contains(".env"), "{}", on_remote(&w));
    assert_eq!(a.read("mock/research/.env"), b"TOKEN=x\n");
}
