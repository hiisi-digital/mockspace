//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a member's tests depend on, reduced to one digest each.
//!
//! **A member's own inputs** are every file under its directory that git
//! tracks or would track, which takes in its sources, its tests, its manifest
//! and whatever it keeps beside them (kaski's shaders sit there), and every
//! path outside its directory that its sources name as a string literal
//! climbing out of it, such as `include_str!("../../../content/x.toml")` or
//! `CARGO_MANIFEST_DIR` joined to `"../../content"`. That second half is how a
//! test's content is found without a list: the path a test reads is written in
//! its source, and it is read the same way here.
//!
//! **A member's fingerprint** is its own inputs, the own inputs of every member
//! compiled into its tests ([`Graph::test_closure`]), and the workspace-wide
//! inputs every build reads: the lockfile, the workspace manifest, the
//! toolchain pin and cargo's and nextest's configuration. The same digest
//! decides whether a member is selected and whether a heavy test's earlier pass
//! still stands, so the two can never disagree about what changed.
//!
//! **What it cannot see** is a path built at run time out of pieces none of
//! which climbs out on its own (`join("..").join("content")`), an environment
//! variable, or a file outside the repository. Each of those is a test that may
//! be skipped when it should have run, and `research/202610091400_suite-selection.md`
//! names them as the open edges.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::graph::Graph;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fingerprints {
    /// Each member's own inputs, as one digest.
    pub own:     BTreeMap<String, String>,
    /// Each member's fingerprint: its own, its closure's and the workspace's.
    pub full:    BTreeMap<String, String>,
    /// The paths outside each member that its sources name, relative to the
    /// repository root, for the report.
    pub reaches: BTreeMap<String, Vec<PathBuf>>,
}

/// Files every build in the workspace reads, relative to the mock directory and
/// to the repository root. Absent ones are skipped.
const WORKSPACE_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".cargo/config",
    ".cargo/config.toml",
    ".config/nextest.toml",
];

pub fn compute(repo_root: &Path, mock_dir: &Path, graph: &Graph) -> Result<Fingerprints, String> {
    let files = tracked_files(repo_root)?;
    let mut hashes = FileHashes::new(repo_root);

    let mut salt = Sha256::new();
    // Named by where they sit relative to the repository, never by absolute
    // path, so the same tree on another machine has the same fingerprints and
    // a pass recorded in the tracked history stands there too.
    for (base, label) in [(mock_dir, "mock"), (repo_root, "root")] {
        for name in WORKSPACE_INPUTS {
            let p = base.join(name);
            if p.is_file() {
                salt.update(format!("{label}/{name}").as_bytes());
                salt.update(hashes.of_absolute(&p));
            }
        }
    }
    let salt = salt.finalize();

    let mut fp = Fingerprints::default();
    for (name, pkg) in &graph.packages {
        let Ok(dir) = pkg.dir.strip_prefix(repo_root) else {
            // A path dependency outside the repository: git cannot list it,
            // so its files are read off the disk, build output aside, and
            // its own literals are not followed.
            let mut h = Sha256::new();
            for f in walk_outside(&pkg.dir) {
                let rel = f.strip_prefix(&pkg.dir).unwrap_or(&f);
                h.update(rel.to_string_lossy().as_bytes());
                h.update([0]);
                h.update(hashes.of_absolute(&f));
            }
            fp.own.insert(name.clone(), hex(&h.finalize()));
            fp.reaches.insert(name.clone(), Vec::new());
            continue;
        };
        let own_files = under(&files, dir);

        let mut reaches: BTreeSet<PathBuf> = BTreeSet::new();
        for f in own_files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "rs"))
        {
            let Ok(src) = std::fs::read_to_string(repo_root.join(f)) else {
                continue;
            };
            let file_dir = f.parent().unwrap_or(Path::new(""));
            for lit in path_literals(&src) {
                if let Some(r) = reach(&lit, file_dir, dir, repo_root) {
                    reaches.insert(r);
                }
            }
        }

        let mut h = Sha256::new();
        for f in &own_files {
            h.update(f.to_string_lossy().as_bytes());
            h.update([0]);
            h.update(hashes.of(f));
        }
        for r in &reaches {
            h.update(b"reach\0");
            h.update(r.to_string_lossy().as_bytes());
            let inside = under(&files, r);
            if inside.is_empty() {
                // A file the source names that git does not list, such as a
                // generated one: read off the disk, or marked absent.
                h.update(hashes.of(r));
            }
            for f in inside {
                h.update(f.to_string_lossy().as_bytes());
                h.update([0]);
                h.update(hashes.of(&f));
            }
        }
        fp.own.insert(name.clone(), hex(&h.finalize()));
        fp.reaches
            .insert(name.clone(), reaches.into_iter().collect());
    }

    for name in graph.packages.keys() {
        let mut h = Sha256::new();
        h.update(salt);
        h.update(fp.own[name].as_bytes());
        for dep in graph.test_closure(name) {
            h.update(dep.as_bytes());
            h.update([0]);
            h.update(
                fp.own
                    .get(&dep)
                    .map(String::as_str)
                    .unwrap_or("")
                    .as_bytes(),
            );
        }
        fp.full.insert(name.clone(), hex(&h.finalize()));
    }
    Ok(fp)
}

/// Every file git tracks or would track, relative to the repository root:
/// tracked, and untracked but not ignored, so an uncommitted new test is an
/// input like a committed one and a build output is not.
fn tracked_files(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let out = Command::new("git")
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("could not run git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-files failed in {}: {}",
            repo_root.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut files: Vec<PathBuf> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned()))
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// Every file under a directory outside the repository, skipping `target`
/// and hidden directories, sorted.
fn walk_outside(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name();
            let skip = name == "target" || name.to_string_lossy().starts_with('.');
            match e.file_type() {
                Ok(t) if t.is_dir() && !skip => stack.push(p),
                Ok(t) if t.is_file() => out.push(p),
                _ => {},
            }
        }
    }
    out.sort();
    out
}

/// The files at or under `dir`, from a sorted list.
fn under(files: &[PathBuf], dir: &Path) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| f.starts_with(dir))
        .cloned()
        .collect()
}

/// Each file's digest once per run, however many members read it.
struct FileHashes<'a> {
    root: &'a Path,
    memo: BTreeMap<PathBuf, [u8; 32]>,
}

impl<'a> FileHashes<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            memo: BTreeMap::new(),
        }
    }

    fn of(&mut self, rel: &Path) -> [u8; 32] {
        let abs = self.root.join(rel);
        self.of_absolute(&abs)
    }

    /// A file that cannot be read, such as one deleted but still in the index,
    /// hashes as a fixed marker, which differs from any content it had.
    fn of_absolute(&mut self, abs: &Path) -> [u8; 32] {
        if let Some(h) = self.memo.get(abs) {
            return *h;
        }
        let h: [u8; 32] = match std::fs::read(abs) {
            Ok(bytes) => Sha256::digest(&bytes).into(),
            Err(_) => Sha256::digest(b"\0absent\0").into(),
        };
        self.memo.insert(abs.to_path_buf(), h);
        h
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The string literals in a Rust source that read as relative paths climbing
/// out of where they are resolved: after an optional leading `/`, they start
/// with `..` and name at least one real directory or file.
///
/// Parsed rather than searched, so a path in a comment or a doc string is not
/// an input, and a raw string is read as written.
pub fn path_literals(source: &str) -> Vec<String> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "string_literal" | "raw_string_literal" => {
                if let Some(v) = node
                    .utf8_text(source.as_bytes())
                    .ok()
                    .and_then(literal_value)
                    .filter(|v| climbs_out(v))
                {
                    out.push(v.to_string());
                }
            },
            _ => {
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            },
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The text between a literal's quotes, or nothing for one with an escape in
/// it, which a path is never written with.
fn literal_value(text: &str) -> Option<&str> {
    let body = text.trim_start_matches('r').trim_start_matches('#');
    let body = body.trim_end_matches('#');
    let inner = body.strip_prefix('"')?.strip_suffix('"')?;
    (!inner.contains('\\')).then_some(inner)
}

fn climbs_out(v: &str) -> bool {
    let p = Path::new(v.trim_start_matches('/'));
    let mut comps = p.components();
    matches!(comps.next(), Some(Component::ParentDir))
        && p.components().any(|c| matches!(c, Component::Normal(_)))
}

/// Where a path literal lands, relative to the repository root, if it lands
/// somewhere outside the member that is worth reading.
///
/// Tried against the source file's own directory, which is what
/// `include_str!` resolves against, and against the member's directory, which
/// is what `CARGO_MANIFEST_DIR` and a test's working directory are. Only a
/// path that exists is taken, which is also what decides between the two.
///
/// Refused: anything inside the member, which its own files already cover;
/// anything containing the member, which would make every file an input; and
/// anything outside the repository or under a `target` directory.
pub fn reach(lit: &str, file_dir: &Path, member_dir: &Path, repo_root: &Path) -> Option<PathBuf> {
    let rel = Path::new(lit.trim_start_matches('/'));
    for base in [file_dir, member_dir] {
        let Some(p) = normalise(&base.join(rel)) else {
            continue;
        };
        if p.as_os_str().is_empty()
            || p.starts_with(member_dir)
            || member_dir.starts_with(&p)
            || p.components().any(|c| c.as_os_str() == "target")
        {
            continue;
        }
        if repo_root.join(&p).exists() {
            return Some(p);
        }
    }
    None
}

/// `a/b/../c` as `a/c`, lexically, or nothing when it climbs above where it
/// started, which here means out of the repository.
fn normalise(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            },
            Component::CurDir => {},
            Component::Normal(n) => out.push(n),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "fingerprint_tests.rs"]
mod tests;
