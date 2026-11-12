//! The copy of an instance is built from the last copy, with only what changed handed to git, so
//! it must come out as a copy built whole would: a file where a directory was, a directory where a
//! file was, and a file gone from the instance; and a first guard, built from the base, must hold
//! every file of the instance.

mod common;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{World, git, inst_dir, seeded, try_git, write};
use mockspace_store::Instance;

/// Every path of the guard set of the backup ref, from the instance, and the bytes of each.
fn guarded(repo: &std::path::Path, i: &Instance<'_>) -> Vec<(String, String)> {
    let reference = i.backup_ref();
    let mut out = Vec::new();
    for path in git(repo, &["ls-tree", "-r", "--name-only", &reference]).lines() {
        let Some((_, rel)) = path.split_once("/guard/") else { continue };
        out.push((rel.to_string(), git(repo, &["show", &format!("{reference}:{path}")])));
    }
    out.sort();
    out
}

// Mutation: seed the copy's index from the last copy and add what changed without taking out what
// is gone, or add a file where the last copy had a directory without replacing it; the guard then
// holds a file the instance no longer has, or the copy fails.
#[test]
fn a_guard_built_from_the_last_holds_what_the_instance_holds_now() {
    let w = World::new();
    let a = seeded(&w);
    let repo = w.dir.path().join("a");
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "shape/inner.md", "a file in a directory\n");
    write(&dir, "flat.md", "a file that becomes a directory\n");
    write(&dir, "gone.md", "a file taken out\n");
    i.guard().unwrap().copied.unwrap();
    let first = guarded(&repo, &i);
    assert!(first.iter().any(|(p, _)| p == "shape/inner.md"), "the control: the first guard holds the file: {first:?}");
    // the first guard is the base and what changed against it, every file of the instance
    let mut on_disk: Vec<String> = ["flat.md", "gone.md", "shape/inner.md", "ticket/t1.toml", "ticket/t2.toml"].map(String::from).to_vec();
    on_disk.sort();
    assert_eq!(first.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), on_disk);
    std::fs::remove_dir_all(dir.join("shape")).unwrap();
    write(&dir, "shape", "a file where the directory was\n");
    std::fs::remove_file(dir.join("flat.md")).unwrap();
    write(&dir, "flat.md/now.md", "a directory where the file was\n");
    std::fs::remove_file(dir.join("gone.md")).unwrap();
    i.guard().unwrap().copied.unwrap();
    let now = guarded(&repo, &i);
    let names: Vec<&str> = now.iter().map(|(p, _)| p.as_str()).collect();
    assert!(names.contains(&"shape") && !names.contains(&"shape/inner.md"), "{now:?}");
    assert!(names.contains(&"flat.md/now.md") && !names.contains(&"flat.md"), "{now:?}");
    assert!(!names.contains(&"gone.md"), "{now:?}");
    let shape = now.iter().find(|(p, _)| p == "shape").unwrap();
    assert_eq!(shape.1, "a file where the directory was");
}

/// A file as git keeps it: its mode and its blob.
type Id = (String, String);
type Files = BTreeMap<String, Id>;

/// Every file of the instance on disk, by path from it.
fn on_disk(repo: &Path, dir: &Path) -> Files {
    let mut out = Files::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            let exec = std::os::unix::fs::PermissionsExt::mode(&e.metadata().unwrap().permissions()) & 0o111 != 0;
            let blob = git(repo, &["hash-object", "--no-filters", p.to_str().unwrap()]);
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
            out.insert(rel, (if exec { "100755" } else { "100644" }.to_string(), blob));
        }
    }
    out
}

/// The files of a tree, by path.
fn tree_files(repo: &Path, tree: &str) -> Files {
    git(repo, &["ls-tree", "-r", tree])
        .lines()
        .filter_map(|l| {
            let (meta, path) = l.split_once('\t')?;
            let mut f = meta.split(' ');
            let mode = f.next()?.to_string();
            let blob = f.nth(1)?.to_string();
            Some((path.to_string(), (mode, blob)))
        })
        .collect()
}

/// The tree of `files` built whole, in an index of its own, or `None` for no files.
fn whole_tree(repo: &Path, files: &Files) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let index = repo.join(".git").join("whole-build.idx");
    let _ = std::fs::remove_file(&index);
    let lines: String = files.iter().map(|(p, (m, b))| format!("{m} {b}\t{p}\n")).collect();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["update-index", "--add", "--index-info"])
        .env("GIT_INDEX_FILE", &index)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(lines.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let tree = Command::new("git").arg("-C").arg(repo).arg("write-tree").env("GIT_INDEX_FILE", &index).output().unwrap();
    let _ = std::fs::remove_file(&index);
    assert!(tree.status.success(), "{}", String::from_utf8_lossy(&tree.stderr));
    Some(String::from_utf8(tree.stdout).unwrap().trim().to_string())
}

/// The name of the one instance's record, which is its directory in the backup ref.
fn record_key(repo: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(repo.join(".git/mock-instances"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.contains('.'))
        .collect();
    assert_eq!(names.len(), 1, "one instance: {names:?}");
    names.remove(0)
}

/// What the three sets of the copy are to hold, worked out from the instance and the base alone.
struct Model {
    base:  Files,
    guard: Files,
    lost:  Files,
}

impl Model {
    fn new(repo: &Path) -> Self {
        let record = std::fs::read_to_string(repo.join(".git/mock-instances").join(record_key(repo))).unwrap();
        let base = record.split('\t').next().unwrap().to_string();
        Model { base: tree_files(repo, &base), guard: Files::new(), lost: Files::new() }
    }

    /// The sets after a look, or a guard where `guard`: a file of the last guard gone from disk is
    /// one git wrote over, kept as the guard had it until the disk holds it again; the edit is every
    /// file that differs from the base; the guard is the disk where one is taken, and empty after a
    /// look.
    fn after(&mut self, disk: &Files, guard: bool) -> Files {
        for (path, id) in &self.guard {
            if !disk.contains_key(path) && !self.lost.contains_key(path) {
                self.lost.insert(path.clone(), id.clone());
            }
        }
        self.lost.retain(|path, id| disk.get(path) != Some(id));
        self.guard = if guard { disk.clone() } else { Files::new() };
        disk.iter().filter(|(p, id)| self.base.get(*p) != Some(*id)).map(|(p, id)| (p.clone(), id.clone())).collect()
    }
}

/// Each set of the backup ref against the same set built whole from the model.
fn matches_whole(repo: &Path, i: &Instance<'_>, model: &Model, edit: &Files, at: &str) {
    let key = record_key(repo);
    for (set, files) in [("edit", edit), ("guard", &model.guard), ("lost", &model.lost)] {
        let built = try_git(repo, &["rev-parse", "--verify", "-q", &format!("{}:{key}/{set}", i.backup_ref())]).ok();
        assert_eq!(built, whole_tree(repo, files), "{at}: the `{set}` set built from the last copy differs from one built whole of {files:?}");
    }
}

// Mutation: hand git only the added paths and not the removals, seed a set from another set of the
// last copy, or leave a set of the last copy in place where the new one is empty; a set then holds
// what a whole build of it does not.
#[test]
fn every_set_built_from_the_last_copy_is_the_set_built_whole() {
    let w = World::new();
    let a = seeded(&w);
    let repo = w.dir.path().join("a");
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let mut m = Model::new(&repo);
    let step = |what: &str, guard: bool, m: &mut Model| {
        if guard { i.guard() } else { i.look() }.unwrap().copied.unwrap();
        let edit = m.after(&on_disk(&repo, &dir), guard);
        matches_whole(&repo, &i, m, &edit, what);
    };
    write(&dir, "shape/inner.md", "a file in a directory\n");
    write(&dir, "flat.md", "a file that becomes a directory\n");
    write(&dir, "gone.md", "a file git writes over\n");
    write(&dir, "ticket/t1.toml", "id = \"t1\"\nedited = true\n");
    step("a look with edits only", false, &mut m);
    step("a guard after a copy with no guard", true, &mut m);
    std::fs::remove_file(dir.join("gone.md")).unwrap();
    std::fs::remove_file(dir.join("ticket/t2.toml")).unwrap();
    std::fs::remove_dir_all(dir.join("shape")).unwrap();
    write(&dir, "shape", "a file where the directory was\n");
    std::fs::remove_file(dir.join("flat.md")).unwrap();
    write(&dir, "flat.md/now.md", "a directory where the file was\n");
    step("a look after git wrote over guarded files", false, &mut m);
    assert!(!m.lost.is_empty(), "the control: the look found files written over");
    write(&dir, "later.md", "an edit after the loss\n");
    step("a look that keeps the lost set", false, &mut m);
    write(&dir, "gone.md", "a file git writes over\n");
    step("a look after a lost file holds its bytes again", false, &mut m);
    step("a guard over a lost set", true, &mut m);
    std::fs::remove_file(dir.join("later.md")).unwrap();
    step("a guard after an edit taken out", true, &mut m);
}

// Mutation: build from the last copy the ref holds and pass over the pending one, or take the
// pending one's tree under another key; the look then drops the guard the pending copy kept, and
// with it what git wrote over.
#[test]
fn a_copy_built_from_a_pending_copy_is_the_copy_built_whole() {
    let w = World::new();
    let a = seeded(&w);
    let repo = w.dir.path().join("a");
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let mut m = Model::new(&repo);
    write(&dir, "first.md", "an edit\n");
    i.look().unwrap().copied.unwrap();
    m.after(&on_disk(&repo, &dir), false);
    let tip = git(&repo, &["rev-parse", &i.backup_ref()]);
    // the ref is locked, as a reference transaction holds it while its hook runs
    let lock = repo.join(".git").join(format!("{}.lock", i.backup_ref()));
    std::fs::write(&lock, "").unwrap();
    write(&dir, "second.md", "an edit the guard keeps\n");
    i.guard().unwrap().copied.unwrap();
    std::fs::remove_file(&lock).unwrap();
    m.after(&on_disk(&repo, &dir), true);
    let pending = repo.join(".git/mock-instances").join(format!("{}.guard", record_key(&repo)));
    assert!(pending.is_file(), "the control: the guard is kept beside the record");
    assert_eq!(git(&repo, &["rev-parse", &i.backup_ref()]), tip, "the control: the ref did not take it");
    std::fs::remove_file(dir.join("first.md")).unwrap();
    write(&dir, "third.md", "an edit after the guard\n");
    i.look().unwrap().copied.unwrap();
    let edit = m.after(&on_disk(&repo, &dir), false);
    assert!(m.lost.contains_key("first.md"), "the control: the pending guard's file gone is written over");
    assert!(!pending.exists(), "the look took the pending copy into the ref");
    matches_whole(&repo, &i, &m, &edit, "a look from a pending copy");
}

// Mutation: seed the guard from the last copy's empty guard and not from the base; the guard then
// holds only the files that differ from the base.
#[test]
fn a_guard_after_a_copy_that_had_none_is_every_file_of_the_instance() {
    let w = World::new();
    let a = seeded(&w);
    let repo = w.dir.path().join("a");
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let mut m = Model::new(&repo);
    write(&dir, "added.md", "an edit\n");
    std::fs::remove_file(dir.join("ticket/t2.toml")).unwrap();
    i.look().unwrap().copied.unwrap();
    m.after(&on_disk(&repo, &dir), false);
    let key = record_key(&repo);
    assert!(
        try_git(&repo, &["rev-parse", "--verify", "-q", &format!("{}:{key}/guard", i.backup_ref())]).is_err(),
        "the control: the last copy has no guard"
    );
    i.guard().unwrap().copied.unwrap();
    let edit = m.after(&on_disk(&repo, &dir), true);
    assert!(m.guard.contains_key("ticket/t1.toml") && !m.guard.contains_key("ticket/t2.toml"), "{:?}", m.guard);
    matches_whole(&repo, &i, &m, &edit, "a guard after a copy with none");
}
