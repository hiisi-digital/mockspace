//! The whole of a repository as a person has it: every file below the root but `.git`, ignored
//! ones included and byte for byte, and every reference, in the clone and on the remote. A check
//! over it says what a command lost, wherever the loss was.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;

use super::world::{Clone, World};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Entry {
    File { exec: bool, bytes: Vec<u8> },
    Link(Vec<u8>),
}

/// Path from the root, as bytes, so a name that is not UTF-8 is kept as it is.
pub type Disk = BTreeMap<Vec<u8>, Entry>;

/// Reference name to the object it is at.
pub type Refs = BTreeMap<String, String>;

pub fn shown(p: &[u8]) -> String {
    String::from_utf8_lossy(p).into_owned()
}

/// Every file and link below `root`, the `.git` at the top left out.
pub fn disk(root: &Path) -> Disk {
    let mut out = Disk::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

fn walk(dir: &Path, rel: &mut Vec<u8>, out: &mut Disk) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        if rel.is_empty() && name == ".git" {
            continue;
        }
        let mut here = rel.clone();
        if !here.is_empty() {
            here.push(b'/');
        }
        here.extend_from_slice(name.as_bytes());
        let meta = std::fs::symlink_metadata(e.path()).unwrap();
        if meta.file_type().is_symlink() {
            out.insert(here, Entry::Link(std::fs::read_link(e.path()).unwrap().as_os_str().as_bytes().to_vec()));
        } else if meta.is_dir() {
            walk(&e.path(), &mut here, out);
        } else {
            let exec = meta.permissions().mode() & 0o111 != 0;
            out.insert(here, Entry::File { exec, bytes: std::fs::read(e.path()).unwrap() });
        }
    }
}

pub fn refs(w: &World, git_dir: &Path) -> Refs {
    let o = w.sh(git_dir, "git", &["for-each-ref", "--format=%(objectname) %(refname)"]);
    assert!(o.ok, "{}", o.all());
    o.stdout.lines().filter_map(|l| l.split_once(' ')).map(|(id, name)| (name.to_string(), id.to_string())).collect()
}

pub struct Whole {
    pub disk:   Disk,
    pub local:  Refs,
    pub remote: Refs,
}

pub fn capture(c: &Clone) -> Whole {
    Whole { disk: disk(&c.root), local: refs(c.w, &c.root), remote: refs(c.w, &c.w.remote) }
}

/// What a command may change on disk and still have lost nothing.
#[derive(Default)]
pub struct Allowed {
    /// Moved from the first path to the second, bytes and mode as they were.
    pub renamed:   Vec<(Vec<u8>, Vec<u8>)>,
    /// Written with other bytes, where the remote keeps the bytes from before.
    pub rewritten: BTreeSet<Vec<u8>>,
}

/// The id git gives `bytes` as a blob.
pub fn blob_of(w: &World, bytes: &[u8]) -> String {
    let mut child = w
        .command("git", &w.base())
        .args(["hash-object", "--stdin", "--no-filters"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().trim().to_string()
}

/// Every object reachable from a reference or a reflog entry of the repository at `dir`.
pub fn reachable(w: &World, dir: &Path) -> BTreeSet<String> {
    let o = w.sh(dir, "git", &["rev-list", "--objects", "--all", "--reflog"]);
    assert!(o.ok, "{}", o.all());
    o.stdout.lines().filter_map(|l| l.split(' ').next()).map(str::to_owned).collect()
}

/// Every path of `before` is in `after` with the same bytes, mode and link target, at its new path
/// where it was renamed; a rewritten one may differ where a commit holds what it was; nothing is
/// on disk after that was not before, but the renamed paths; and every reference, in the clone and on
/// the remote, is where it was or ahead of it. Returns every violation at once.
pub fn kept(c: &Clone, before: &Whole, after: &Whole, allowed: &Allowed) -> Result<(), String> {
    let mut bad = Vec::new();
    let to: BTreeMap<&[u8], &[u8]> = allowed.renamed.iter().map(|(f, t)| (f.as_slice(), t.as_slice())).collect();
    let mut on_remote: Option<BTreeSet<String>> = None;
    for (path, was) in &before.disk {
        let now_at = to.get(path.as_slice()).copied().unwrap_or(path);
        match after.disk.get(now_at) {
            Some(now) if now == was => {},
            Some(Entry::File { .. }) if allowed.rewritten.contains(path) => {
                let Entry::File { bytes, .. } = was else {
                    bad.push(format!("{}: a link was rewritten", shown(path)));
                    continue;
                };
                let ids = on_remote.get_or_insert_with(|| {
                    let mut all = reachable(c.w, &c.w.remote);
                    all.extend(reachable(c.w, &c.root));
                    all
                });
                if !ids.contains(&blob_of(c.w, bytes)) {
                    bad.push(format!("{}: rewritten, and no commit here or on the remote holds what it was", shown(path)));
                }
            },
            Some(now) => bad.push(format!("{}: changed, {} became {}", shown(path), brief(was), brief(now))),
            None => bad.push(format!("{}: gone from disk", shown(path))),
        }
    }
    let renamed_to: BTreeSet<&[u8]> = to.values().copied().collect();
    for path in after.disk.keys().filter(|p| !before.disk.contains_key(*p) && !renamed_to.contains(p.as_slice()) && !engine_output(p)) {
        bad.push(format!("{}: appeared", shown(path)));
    }
    for (side, was, now, dir) in [("here", &before.local, &after.local, c.root.as_path()), ("remote", &before.remote, &after.remote, c.w.remote.as_path())] {
        for (name, old) in was {
            match now.get(name) {
                None => bad.push(format!("{side} {name}: gone, it was at {old}")),
                Some(new) if new == old => {},
                Some(new) => {
                    if !c.w.sh(dir, "git", &["merge-base", "--is-ancestor", old, new]).ok {
                        bad.push(format!("{side} {name}: moved from {old} to {new}, which does not hold it"));
                    }
                },
            }
        }
    }
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

/// What the engine writes for itself on a run that lints: its build directories and the taplo
/// config at the root. It appearing is no change to anybody's data.
fn engine_output(p: &[u8]) -> bool {
    let p = shown(p);
    p == ".taplo.toml" || p.starts_with("target/") || p.contains("/target/")
}

fn brief(e: &Entry) -> String {
    match e {
        Entry::File { exec, bytes } => format!("file of {} bytes, exec {exec}", bytes.len()),
        Entry::Link(t) => format!("link to {}", shown(t)),
    }
}

/// What a person wrote during a scenario. The check at the end is that every text is somewhere a
/// person gets it back from: on the disk of a clone, or reachable from a reference or a reflog of a
/// clone or of the remote. An object only a pending copy names, which `git gc` may take, is not.
#[derive(Default)]
pub struct Ledger {
    texts: Vec<(String, Vec<u8>)>,
}

impl Ledger {
    pub fn write(&mut self, c: &Clone, rel: &str, bytes: &[u8]) {
        c.write(rel, bytes);
        self.texts.push((format!("{rel} in {}", c.root.file_name().unwrap().to_string_lossy()), bytes.to_vec()));
    }

    pub fn kept(&self, w: &World, clones: &[&Clone]) -> Result<(), String> {
        let mut ids = reachable(w, &w.remote);
        let mut on_disk = BTreeSet::new();
        for c in clones {
            ids.extend(reachable(w, &c.root));
            for e in disk(&c.root).into_values() {
                if let Entry::File { bytes, .. } = e {
                    on_disk.insert(bytes);
                }
            }
        }
        let lost: Vec<String> = self
            .texts
            .iter()
            .filter(|(_, b)| !on_disk.contains(b) && !ids.contains(&blob_of(w, b)))
            .map(|(what, b)| format!("{what}: {:?}", String::from_utf8_lossy(b)))
            .collect();
        if lost.is_empty() { Ok(()) } else { Err(format!("written and now nowhere:\n{}", lost.join("\n"))) }
    }
}

/// Paths under `prefixes` that a commit of `range` on the branch tracks: an instance file committed
/// to the branch by mistake.
pub fn held_on_the_branch(c: &Clone, range: &str, prefixes: &[&str]) -> Vec<String> {
    let commits = c.git(&["rev-list", range]);
    let mut out = BTreeSet::new();
    for commit in commits.lines() {
        for p in c.git(&["ls-tree", "-r", "-z", "--name-only", commit]).split('\0').filter(|p| !p.is_empty()) {
            if prefixes.iter().any(|pre| p.starts_with(pre)) {
                out.insert(format!("{commit}: {p}"));
            }
        }
    }
    out.into_iter().collect()
}
