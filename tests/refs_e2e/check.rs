//! The data-loss property: every file tracked before is afterwards, byte for byte and mode for
//! mode, either on the branch or on a ref, and nowhere else is it silently changed.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::world::{Clone, World};

/// path from the repository root to `(mode, blob id)`.
pub type Manifest = BTreeMap<String, (String, String)>;

/// One declared ref: its namespace and the directory under `mock/` it holds.
#[derive(Clone, Copy, Debug)]
pub struct Held {
    pub ns:   &'static str,
    pub path: &'static str,
}

fn parse_tree(text: &str, prefix: &str) -> Manifest {
    text.split('\0')
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (meta, path) = l.split_once('\t').expect("ls-tree line");
            let mut it = meta.split(' ');
            let mode = it.next().unwrap().to_string();
            let _kind = it.next();
            let oid = it.next().unwrap().to_string();
            (format!("{prefix}{path}"), (mode, oid))
        })
        .collect()
}

/// What the branch holds at HEAD.
pub fn branch(c: &Clone) -> Manifest {
    parse_tree(&c.git(&["ls-tree", "-r", "-z", "HEAD"]), "")
}

/// What a commit holds, for the state before a change.
pub fn at(c: &Clone, rev: &str) -> Manifest {
    parse_tree(&c.git(&["ls-tree", "-r", "-z", rev]), "")
}

/// What the refs hold on the remote, mapped to repository-root paths. A ref not there holds nothing.
pub fn held_on_remote(w: &World, refs: &[Held]) -> Manifest {
    held_on_remote_at(w, refs, "mock")
}

/// [`held_on_remote`] for a mock directory at `mock` from the root.
pub fn held_on_remote_at(w: &World, refs: &[Held], mock: &str) -> Manifest {
    let mut all = Manifest::new();
    for h in refs {
        let reference = format!("refs/heads/mock/{}", h.ns);
        if w.remote_tip(&reference).is_none() {
            continue;
        }
        let text = w.remote_git(&["ls-tree", "-r", "-z", &reference]);
        all.extend(parse_tree(&text, &format!("{mock}/{}/", h.path)));
    }
    all
}

/// Every path of `before` is, identical, in exactly one of the branch and the refs, and nothing
/// that was not before is held by a ref. Returns every violation at once.
pub fn no_loss(before: &Manifest, branch: &Manifest, held: &Manifest, except: &[&str]) -> Result<(), String> {
    let mut bad = Vec::new();
    for (path, want) in before.iter().filter(|(p, _)| !except.contains(&p.as_str())) {
        match (branch.get(path), held.get(path)) {
            (Some(b), None) if b == want => {},
            (None, Some(h)) if h == want => {},
            (Some(_), Some(_)) => bad.push(format!("{path}: on the branch and on a ref both")),
            (None, None) => bad.push(format!("{path}: on neither the branch nor a ref")),
            (Some(b), None) => bad.push(format!("{path}: changed on the branch, {want:?} became {b:?}")),
            (None, Some(h)) => bad.push(format!("{path}: changed on the ref, {want:?} became {h:?}")),
        }
    }
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

/// Every entry of `held` is on disk in `c` with the same content, executable bit and link target.
pub fn instance_matches(c: &Clone, held: &Manifest, modes: bool) -> Result<(), String> {
    let mut bad = Vec::new();
    for (path, (mode, oid)) in held {
        let p = c.path(path);
        let Ok(meta) = fs::symlink_metadata(&p) else {
            bad.push(format!("{path}: missing from disk"));
            continue;
        };
        if mode == "120000" {
            let want = c.w.sh(&c.root, "git", &["cat-file", "blob", oid]).stdout;
            match fs::read_link(&p) {
                Ok(t) if t.to_string_lossy() == want => {},
                other => bad.push(format!("{path}: link is {other:?}, wanted {want:?}")),
            }
            continue;
        }
        if !meta.is_file() {
            bad.push(format!("{path}: not a file on disk"));
            continue;
        }
        let exec = meta.permissions().mode() & 0o111 != 0;
        if modes && exec != (mode == "100755") {
            bad.push(format!("{path}: executable bit is {exec}, ref mode {mode}"));
        }
        let got = c.w.sh(&c.root, "git", &["hash-object", "--no-filters", p.to_str().unwrap()]).stdout;
        if got.trim() != oid {
            bad.push(format!("{path}: content differs from the ref ({} vs {oid})", got.trim()));
        }
    }
    if bad.is_empty() { Ok(()) } else { Err(bad.join("\n")) }
}

/// A file's id as git would store it, for comparing a disk file with a recorded one.
pub fn blob_id(c: &Clone, rel: &str) -> String {
    c.w.sh(&c.root, "git", &["hash-object", "--no-filters", c.path(rel).to_str().unwrap()]).stdout.trim().to_string()
}

/// The same manifest with the executable bit folded away, for the checks that are about content
/// and paths and must not be hidden behind the one about modes.
pub fn content_only(m: &Manifest) -> Manifest {
    m.iter()
        .map(|(p, (mode, oid))| (p.clone(), (if mode == "100755" { "100644".to_string() } else { mode.clone() }, oid.clone())))
        .collect()
}

/// Paths a ref holds that the branch never tracked: ignored or untracked files swept in.
pub fn strays(before: &Manifest, held: &Manifest) -> Vec<String> {
    held.keys().filter(|p| !before.contains_key(*p)).cloned().collect()
}
