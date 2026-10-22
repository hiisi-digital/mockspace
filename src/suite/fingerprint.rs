//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a member's tests depend on, reduced to digests.
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
//! **A member's shared inputs** are its own less the files only one of its
//! integration-test binaries is built from ([`super::units`] says which, and why
//! that is sound). What another member's tests compile of it is its library and
//! what the library needs, never its `tests/`, so its shared inputs are what
//! reaches a dependent.
//!
//! **A member's fingerprint** is its own inputs, the shared inputs of every
//! member compiled into its tests ([`Graph::test_closure`]), and the
//! workspace-wide inputs every build reads: the lockfile, the workspace
//! manifest, the toolchain pin and cargo's and nextest's configuration. It
//! decides whether a member is selected.
//!
//! **A test binary's fingerprint** is the member's shared inputs, that binary's
//! unit (or every unit, for one that may read any of them), the shared inputs of
//! the closure, and the workspace's. It decides whether a binary whose member is
//! selected still needs to run, and whether a heavy test's earlier pass still
//! stands. Whatever the member builds that is not an integration test (its
//! library's own tests, its binaries and examples) shares one fingerprint, the
//! member's base.
//!
//! **What it cannot see** is a path built at run time out of pieces none of
//! which climbs out on its own (`join("..").join("content")`), an environment
//! variable's value, or a file outside the repository. Each of those is a test
//! that may be skipped when it should have run, and
//! `research/202610091400_suite-selection.md` names them as the open edges.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::graph::Graph;
use super::units::{self, Facts, Index, Layout};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fingerprints {
    /// Each member's own inputs, as one digest.
    pub own:     BTreeMap<String, String>,
    /// Each member's inputs less its integration tests' own files: what its
    /// library and every binary of it are built from, and what a dependent sees.
    pub shared:  BTreeMap<String, String>,
    /// Each member's fingerprint: its own, its closure's shared and the
    /// workspace's.
    pub full:    BTreeMap<String, String>,
    /// Each integration-test binary's fingerprint, by binary id.
    pub bins:    BTreeMap<String, String>,
    /// The fingerprint of whatever a member builds that is not an integration
    /// test: its library's unit tests, its binaries and its examples.
    pub base:    BTreeMap<String, String>,
    /// The paths outside each member that its sources name, relative to the
    /// repository root, for the report.
    pub reaches: BTreeMap<String, Vec<PathBuf>>,
}

impl Fingerprints {
    /// The fingerprint a test binary stands or falls at: its own when it is an
    /// integration test, else its member's base.
    pub fn of_binary(&self, binary: &str) -> Option<&String> {
        self.bins
            .get(binary)
            .or_else(|| self.base.get(super::junit::package_of(binary)))
    }
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

/// Bumped when what the digests are made of changes, so a record kept under the
/// old scheme never equals one made under the new.
const SCHEME: &str = "2";

/// `cargo_config` is the generated cargo config the run builds with, if any: a
/// member's opt level changes what its tests are built from, so it salts every
/// fingerprint as a workspace input does. Absent, the salt is what it was
/// before the config existed, and the pass already on record stands.
pub fn compute(
    repo_root: &Path,
    mock_dir: &Path,
    graph: &Graph,
    cargo_config: Option<&str>,
) -> Result<Fingerprints, String> {
    let files = tracked_files(repo_root)?;
    let index = Index::new(&files);
    let mut hashes = FileHashes::new(repo_root);

    let mut salt = Sha256::new();
    salt.update(format!("scheme\0{SCHEME}\0").as_bytes());
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
    if let Some(text) = cargo_config {
        salt.update(b"cargo-config\0");
        salt.update(text.as_bytes());
    }
    let salt = salt.finalize();

    let mut fp = Fingerprints::default();
    let mut layouts: BTreeMap<String, Layout> = BTreeMap::new();
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
            let digest = hex(&h.finalize());
            fp.own.insert(name.clone(), digest.clone());
            fp.shared.insert(name.clone(), digest);
            fp.reaches.insert(name.clone(), Vec::new());
            continue;
        };
        let own_files = under(&files, dir);

        let mut facts: BTreeMap<PathBuf, Facts> = BTreeMap::new();
        let mut reaches: BTreeSet<PathBuf> = BTreeSet::new();
        for f in own_files
            .iter()
            .filter(|f| f.extension().is_some_and(|e| e == "rs"))
        {
            let Ok(src) = std::fs::read_to_string(repo_root.join(f)) else {
                continue;
            };
            let facts_of = units::facts(&src);
            let file_dir = f.parent().unwrap_or(Path::new(""));
            for lit in &facts_of.climbing {
                if let Some(r) = reach(lit, file_dir, dir, repo_root) {
                    reaches.insert(r);
                }
            }
            facts.insert(f.clone(), facts_of);
        }

        // Which of the member's files belong to one integration-test binary.
        // A manifest that cannot be read, or tells nothing narrowable, leaves
        // every file shared.
        let layout = std::fs::read_to_string(pkg.dir.join("Cargo.toml"))
            .ok()
            .and_then(|m| units::roots(&m, dir, &index))
            .map(|roots| {
                units::layout(
                    &units::Ctx {
                        repo_root,
                        member_dir: dir,
                        index: &index,
                        facts: &facts,
                    },
                    &roots,
                )
            })
            .unwrap_or_default();

        let mut h = Sha256::new();
        for f in &own_files {
            h.update(f.to_string_lossy().as_bytes());
            h.update([0]);
            h.update(hashes.of(f));
        }
        hash_reaches(&mut h, &reaches, &files, &mut hashes);
        fp.own.insert(name.clone(), hex(&h.finalize()));

        let mut h = Sha256::new();
        h.update(b"shared\0");
        for f in own_files
            .iter()
            .filter(|f| !layout.claimed.contains(*f) || layout.shared_pulls.contains(*f))
        {
            h.update(f.to_string_lossy().as_bytes());
            h.update([0]);
            h.update(hashes.of(f));
        }
        // With no unit to tell them apart, every read is the shared sources'.
        let shared_reaches =
            if layout.units.is_empty() { &reaches } else { &layout.shared_reaches };
        hash_reaches(&mut h, shared_reaches, &files, &mut hashes);
        fp.shared.insert(name.clone(), hex(&h.finalize()));
        fp.reaches
            .insert(name.clone(), reaches.into_iter().collect());
        layouts.insert(name.clone(), layout);
    }

    for name in graph.packages.keys() {
        let closure: Vec<(String, String)> = graph
            .test_closure(name)
            .into_iter()
            .map(|dep| {
                let digest = fp.shared.get(&dep).cloned().unwrap_or_default();
                (dep, digest)
            })
            .collect();
        let fold = |h: &mut Sha256| {
            for (dep, digest) in &closure {
                h.update(dep.as_bytes());
                h.update([0]);
                h.update(digest.as_bytes());
            }
        };

        let mut h = Sha256::new();
        h.update(salt);
        h.update(fp.own[name].as_bytes());
        fold(&mut h);
        fp.full.insert(name.clone(), hex(&h.finalize()));

        let mut h = Sha256::new();
        h.update(salt);
        h.update(b"base\0");
        h.update(fp.shared[name].as_bytes());
        fold(&mut h);
        fp.base.insert(name.clone(), hex(&h.finalize()));

        let Some(layout) = layouts.get(name) else {
            continue;
        };
        // What a binary that may read any claimed file is rebuilt on.
        let mut every = Sha256::new();
        for f in &layout.claimed {
            every.update(f.to_string_lossy().as_bytes());
            every.update([0]);
            every.update(hashes.of(f));
        }
        let every_reach: BTreeSet<PathBuf> = layout
            .units
            .values()
            .flat_map(|u| u.reaches.iter().cloned())
            .collect();
        hash_reaches(&mut every, &every_reach, &files, &mut hashes);
        let every = every.finalize();
        for (target, unit) in &layout.units {
            let mut h = Sha256::new();
            h.update(salt);
            h.update(format!("bin\0{target}\0").as_bytes());
            h.update(fp.shared[name].as_bytes());
            if unit.wide {
                h.update(b"wide\0");
                h.update(every);
            } else {
                for f in &unit.files {
                    h.update(f.to_string_lossy().as_bytes());
                    h.update([0]);
                    h.update(hashes.of(f));
                }
                hash_reaches(&mut h, &unit.reaches, &files, &mut hashes);
            }
            fold(&mut h);
            fp.bins
                .insert(format!("{name}::{target}"), hex(&h.finalize()));
        }
    }
    Ok(fp)
}

/// The contents behind each path a source reaches outside its member: the files
/// under it that git lists, else the file itself read off the disk (a generated
/// one), or marked absent.
fn hash_reaches(
    h: &mut Sha256,
    reaches: &BTreeSet<PathBuf>,
    files: &[PathBuf],
    hashes: &mut FileHashes,
) {
    for r in reaches {
        h.update(b"reach\0");
        h.update(r.to_string_lossy().as_bytes());
        let inside = under(files, r);
        if inside.is_empty() {
            h.update(hashes.of(r));
        }
        for f in inside {
            h.update(f.to_string_lossy().as_bytes());
            h.update([0]);
            h.update(hashes.of(&f));
        }
    }
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
    units::facts(source).climbing
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
pub(super) fn normalise(p: &Path) -> Option<PathBuf> {
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

#[cfg(test)]
#[path = "fingerprint_units_tests.rs"]
mod unit_tests;
