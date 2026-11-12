//! The whole of a repository as a person has it: every file below the root but `.git`, ignored
//! ones included and byte for byte, and every reference, in the clone and on the remote. A check
//! over it says what a command lost, wherever the loss was.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
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
    // a directory that cannot be read is no empty directory: the check would pass over all of it
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
    for e in entries {
        let e = e.unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()));
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
    pub git:    Own,
}

/// What the person keeps in the repository beside the files and the references.
#[derive(Debug)]
pub struct Own {
    /// What HEAD names, a branch or `detached`, and the commit it is at, empty where unborn.
    pub head:   (String, String),
    /// Each entry of the index, by path and stage, to its mode and object.
    pub index:  BTreeMap<(Vec<u8>, String), (String, String)>,
    /// Every key and value of the repository's own config and its worktree's, as often as set.
    pub config: Vec<(String, String)>,
    /// The hooks git reads from the repository's hooks directory, the samples left out.
    pub hooks:  BTreeMap<Vec<u8>, Entry>,
    /// The lines of the sparse checkout outside the engine's block, where it is on.
    pub sparse: Option<Vec<String>>,
}

/// The config keys the engine sets for itself, each for a reason it says when it does: the full
/// pattern mode the instances need, and the two keys per-worktree config moves into the main
/// worktree's own file, read the same there.
const ENGINE_KEYS: [&str; 3] = ["core.sparsecheckoutcone", "core.bare", "core.worktree"];

/// The key the gate saves the person's `core.hooksPath` under when it takes the key, which
/// `deactivate` puts back.
const SAVED_HOOKS_PATH: &str = "mockspace.previoushookspath";

/// The start and end of the engine's block in a sparse-checkout file.
const SPARSE_BLOCK: (&str, &str) = ("# mockspace data refs:", "# end mockspace data refs");

fn raw(c: &Clone, args: &[&str]) -> Vec<u8> {
    let o = c.w.command("git", &c.root).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}

fn text(c: &Clone, args: &[&str]) -> Option<String> {
    let o = c.w.sh(&c.root, "git", args);
    o.ok.then(|| o.stdout.trim().to_string())
}

pub fn own(c: &Clone) -> Own {
    let head = (
        text(c, &["symbolic-ref", "-q", "HEAD"]).unwrap_or_else(|| "detached".into()),
        text(c, &["rev-parse", "-q", "--verify", "HEAD"]).unwrap_or_default(),
    );
    let mut index = BTreeMap::new();
    for line in raw(c, &["ls-files", "-s", "-z"]).split(|b| *b == 0).filter(|l| !l.is_empty()) {
        // `<mode> SP <object> SP <stage> TAB <path>`
        let tab = line.iter().position(|b| *b == b'\t').unwrap();
        let meta = String::from_utf8_lossy(&line[.. tab]).into_owned();
        let mut parts = meta.split(' ').map(str::to_owned);
        let (mode, object, stage) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        index.insert((line[tab + 1 ..].to_vec(), stage), (mode, object));
    }
    let listing = c.w.sh(&c.root, "git", &["config", "--list", "--show-scope", "-z"]);
    assert!(listing.ok, "{}", listing.all());
    let mut fields = listing.stdout.split('\0');
    let mut config = Vec::new();
    while let (Some(scope), Some(pair)) = (fields.next(), fields.next()) {
        if scope == "local" || scope == "worktree" {
            let (k, v) = pair.split_once('\n').unwrap_or((pair, ""));
            config.push((k.to_lowercase(), v.to_string()));
        }
    }
    config.sort();
    let common = PathBuf::from(text(c, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).unwrap());
    let hooks_dir = common.join("hooks");
    let hooks = if hooks_dir.exists() {
        disk(&hooks_dir).into_iter().filter(|(p, _)| !p.ends_with(b".sample")).collect()
    } else {
        BTreeMap::new()
    };
    let on = text(c, &["config", "--type=bool", "core.sparseCheckout"]).as_deref() == Some("true");
    let sparse = on.then(|| {
        let file = PathBuf::from(text(c, &["rev-parse", "--path-format=absolute", "--git-path", "info/sparse-checkout"]).unwrap());
        let mut inside = false;
        let mut lines = Vec::new();
        for l in std::fs::read_to_string(&file).unwrap_or_default().lines() {
            if l.starts_with(SPARSE_BLOCK.0) {
                inside = true;
            } else if l == SPARSE_BLOCK.1 {
                inside = false;
            } else if !inside && !l.trim().is_empty() {
                lines.push(l.to_string());
            }
        }
        lines
    });
    Own { head, index, config, hooks, sparse }
}

pub fn capture(c: &Clone) -> Whole {
    Whole { disk: disk(&c.root), local: refs(c.w, &c.root), remote: refs(c.w, &c.w.remote), git: own(c) }
}

/// What a command may change on disk and still have lost nothing.
#[derive(Default)]
pub struct Allowed {
    /// Moved from the first path to the second, bytes and mode as they were.
    pub renamed:   Vec<(Vec<u8>, Vec<u8>)>,
    /// Written with other bytes, where the remote keeps the bytes from before.
    pub rewritten: BTreeSet<Vec<u8>>,
    /// HEAD may move ahead on its branch: the scenario makes a commit between the two captures. A
    /// command of the engine, a migration among them, commits nothing to the branch.
    pub commits:   bool,
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

/// Every object reachable from a reference of the repository at `dir`. A reflog is left out: its
/// entries expire, and a person does not look there for what they wrote.
pub fn reachable(w: &World, dir: &Path) -> BTreeSet<String> {
    let o = w.sh(dir, "git", &["rev-list", "--objects", "--all"]);
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
    kept_own(c, &before.git, &after.git, allowed.commits, &mut || {
        on_remote
            .get_or_insert_with(|| {
                let mut all = reachable(c.w, &c.w.remote);
                all.extend(reachable(c.w, &c.root));
                all
            })
            .clone()
    }, &mut bad);
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

/// HEAD names the branch it named, at the commit it was at, or one ahead of it where the scenario
/// commits; every staged object is staged still at its own path, or held by a commit; every config
/// value of the repository's own is set still, but the engine's own keys, and the person's
/// `core.hooksPath` is set still or saved by the gate that took it; every hook of the person's own is
/// there as it was; and every line of a sparse checkout that was on is in it still.
fn kept_own(c: &Clone, was: &Own, now: &Own, commits: bool, reachable: &mut dyn FnMut() -> BTreeSet<String>, bad: &mut Vec<String>) {
    if was.head.0 != now.head.0 {
        bad.push(format!("HEAD: named {}, now {}", was.head.0, now.head.0));
    } else if was.head.1 != now.head.1 && !commits {
        bad.push(format!("HEAD: moved from {} to {} under a command that commits nothing", was.head.1, now.head.1));
    } else if was.head.1 != now.head.1
        && !(was.head.1.is_empty() || c.w.sh(&c.root, "git", &["merge-base", "--is-ancestor", &was.head.1, &now.head.1]).ok)
    {
        bad.push(format!("HEAD: moved from {} to {}, which does not hold it", was.head.1, now.head.1));
    }
    let mut ids: Option<BTreeSet<String>> = None;
    for ((path, stage), (_, object)) in &was.index {
        if now.index.get(&(path.clone(), stage.clone())).is_some_and(|(_, o)| o == object) {
            continue;
        }
        if !ids.get_or_insert_with(&mut *reachable).contains(object) {
            bad.push(format!("index {} stage {stage}: staged {object}, now in no commit and not staged", shown(path)));
        }
    }
    let mut left = now.config.clone();
    for (k, v) in &was.config {
        match left.iter().position(|(nk, nv)| nk == k && nv == v) {
            Some(i) => {
                left.remove(i);
            },
            None if ENGINE_KEYS.contains(&k.as_str()) => {},
            // the gate's own path, or the person's put where `deactivate` gives it back from
            None if k == "core.hookspath"
                && now.config.iter().any(|(nk, nv)| nk == k && mockspace_manifest::gate::is_ours(nv))
                && (mockspace_manifest::gate::is_ours(v) || now.config.iter().any(|(nk, nv)| nk == SAVED_HOOKS_PATH && nv == v)) => {},
            None => bad.push(format!("config {k}: was {v:?}, now {:?}", now.config.iter().filter(|(nk, _)| nk == k).map(|(_, v)| v).collect::<Vec<_>>())),
        }
    }
    for (name, hook) in &was.hooks {
        let engine = matches!(hook, Entry::File { bytes, .. } if String::from_utf8_lossy(bytes).contains("(managed, do not edit)"));
        if !engine && now.hooks.get(name) != Some(hook) {
            bad.push(format!("hook {}: {} became {}", shown(name), brief(hook), now.hooks.get(name).map_or("nothing".into(), brief)));
        }
    }
    if let Some(lines) = &was.sparse {
        match &now.sparse {
            None => bad.push("sparse checkout: was on, now off".into()),
            Some(now_lines) => {
                for l in lines.iter().filter(|l| !now_lines.contains(l)) {
                    bad.push(format!("sparse checkout: the line {l} is gone"));
                }
            },
        }
    }
}

/// What the engine writes for itself on a run that lints: its build directories and the taplo
/// config at the root. It appearing is no change to anybody's data.
pub fn engine_output(p: &[u8]) -> bool {
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
/// person gets it back from: at the path it was written to, or at one a step the scenario states
/// moved it to, in any of the clones, or reachable from a reference of a clone or of the remote. A
/// text at some other path is not where anybody looks, and an object only a reflog or a pending copy
/// names, which an expiry or `git gc` may take, is not either.
#[derive(Default)]
pub struct Ledger {
    texts: Vec<(String, String, Vec<u8>)>,
    moves: Vec<(String, String)>,
}

impl Ledger {
    pub fn write(&mut self, c: &Clone, rel: &str, bytes: &[u8]) {
        c.write(rel, bytes);
        self.texts.push((c.root.file_name().unwrap().to_string_lossy().into_owned(), rel.to_string(), bytes.to_vec()));
    }

    /// A step of the scenario moves what is at `from` to `to`, as a migration says it does.
    pub fn moved(&mut self, from: &str, to: &str) {
        self.moves.push((from.to_string(), to.to_string()));
    }

    /// `rel` and every path the stated moves take it to.
    fn places(&self, rel: &str) -> Vec<String> {
        let mut out = vec![rel.to_string()];
        let mut i = 0;
        while i < out.len() {
            for (f, t) in &self.moves {
                if *f == out[i] && !out.contains(t) {
                    out.push(t.clone());
                }
            }
            i += 1;
        }
        out
    }

    pub fn kept(&self, w: &World, clones: &[&Clone]) -> Result<(), String> {
        let mut ids = reachable(w, &w.remote);
        for c in clones {
            ids.extend(reachable(w, &c.root));
        }
        let at_its_place = |rel: &str, b: &[u8]| {
            self.places(rel).iter().any(|p| clones.iter().any(|c| std::fs::read(c.path(p)).is_ok_and(|on| on == b)))
        };
        let lost: Vec<String> = self
            .texts
            .iter()
            .filter(|(_, rel, b)| !at_its_place(rel, b) && !ids.contains(&blob_of(w, b)))
            .map(|(clone, rel, b)| format!("{rel} in {clone}: {:?}", String::from_utf8_lossy(b)))
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
