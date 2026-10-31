//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The files the compiler opened to make a library, which is what the library was made from.
//!
//! **Cargo's dep-info is the list, and nothing here assembles another.** The `.d` file beside a
//! built library names every source rustc read, an `include_str!` and a `#[path]` module in a
//! subdirectory among them. A kept library carries that list with the digest of each file, and a
//! tree asking for it re-hashes the files first: the library is loaded only where every one of
//! them is what it was built from.
//!
//! A file is named by the root it sits under (the lint directory, a path package) and the path
//! below it, so two trees of one content name the same files. A file under no root is named by its
//! absolute path, which is right for a registry package and for a checkout in cargo's home.
//!
//! The `# env-dep:` lines of the `.d` file are not checked. The build is hermetic against the
//! environment (`build`), so the variables a compile reads are the ones cargo sets from the
//! generated manifest, which the identity holds.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::identity::digest_bytes;

/// A source modified within this long before the build began, or any time after, may have been
/// read in a state other than the one hashed afterwards. The margin is the filesystem's
/// timestamp granularity.
const MODIFIED_MARGIN: Duration = Duration::from_millis(10);

const SEPARATOR: char = '\t';

/// The directories a tree's files are named relative to, by label.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Roots(Vec<(String, PathBuf)>);

impl Roots {
    pub(crate) fn add(&mut self, label: impl Into<String>, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.0.push((label.into(), path));
    }

    fn resolve(&self, label: &str) -> Option<&Path> {
        self.0.iter().find(|(l, _)| l == label).map(|(_, p)| p.as_path())
    }

    /// The name of `file` in this tree: under the deepest root that holds it, else absolute.
    fn name(&self, file: &Path) -> String {
        let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let deepest = self
            .0
            .iter()
            .filter(|(_, root)| file.starts_with(root))
            .max_by_key(|(_, root)| root.components().count());
        match deepest {
            Some((label, root)) => {
                let rel = file.strip_prefix(root).unwrap_or(&file);
                format!("{label}{SEPARATOR}{}", slashed(rel))
            },
            None => format!("abs{SEPARATOR}{}", slashed(&file)),
        }
    }

    fn locate(&self, name: &str) -> Option<PathBuf> {
        let (label, rel) = name.split_once(SEPARATOR)?;
        if label == "abs" {
            return Some(PathBuf::from(rel));
        }
        Some(self.resolve(label)?.join(rel))
    }
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// One file the library was built from, and the digest of its content then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dep {
    name:   String,
    digest: String,
}

/// The sources a rustc dep-info document names. Its first rule is `<output>: <sources>`, with a
/// space in a path written `\ `, and its comment lines are not sources.
pub(crate) fn sources_of(info: &str) -> Vec<PathBuf> {
    let Some(rule) = info.lines().find(|l| !l.starts_with('#') && l.contains(": ")) else {
        return Vec::new();
    };
    let Some((_, list)) = rule.split_once(": ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = list.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                current.push(' ');
                chars.next();
            },
            ' ' => {
                if !current.is_empty() {
                    out.push(PathBuf::from(std::mem::take(&mut current)));
                }
            },
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(PathBuf::from(current));
    }
    out
}

/// The files the library at `dylib` was built from, with their digests, or `None` where the list
/// cannot be trusted: no dep-info, a file unreadable, or one modified since `started`.
///
/// `gen_dir` is where rustc ran, which a relative path in the list is read against. Its files are
/// not inputs, since the identity holds their text, and nor are the build's own outputs, which
/// are made from the inputs and sit in the target directory above the library.
pub(crate) fn record(
    dylib: &Path,
    gen_dir: &Path,
    roots: &Roots,
    started: SystemTime,
) -> Option<Vec<Dep>> {
    let info = std::fs::read_to_string(info_of(dylib)?).ok()?;
    let outside: Vec<PathBuf> = [Some(gen_dir), dylib.parent().and_then(Path::parent)]
        .into_iter()
        .flatten()
        .map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf()))
        .collect();
    let mut deps = Vec::new();
    for source in sources_of(&info) {
        let file = if source.is_absolute() { source } else { gen_dir.join(source) };
        let seen = file.canonicalize().unwrap_or_else(|_| file.clone());
        if outside.iter().any(|dir| seen.starts_with(dir)) {
            continue;
        }
        let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok()?;
        if modified + MODIFIED_MARGIN >= started {
            return None;
        }
        let name = roots.name(&file);
        if name.contains('\n') {
            return None;
        }
        deps.push(Dep { name, digest: digest_bytes(&std::fs::read(&file).ok()?) });
    }
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    deps.dedup();
    Some(deps)
}

/// The dep-info beside a library: cargo leaves `libname.so` and `libname.d` together.
fn info_of(dylib: &Path) -> Option<PathBuf> {
    Some(dylib.with_extension("d"))
}

/// Whether every file is, in this tree, what the library was built from.
pub(crate) fn hold(deps: &[Dep], roots: &Roots) -> bool {
    deps.iter().all(|dep| {
        roots
            .locate(&dep.name)
            .and_then(|path| std::fs::read(path).ok())
            .is_some_and(|bytes| digest_bytes(&bytes) == dep.digest)
    })
}

pub(crate) fn encode(deps: &[Dep]) -> String {
    let mut out = String::from("mockspace-pack-deps-1\n");
    for dep in deps {
        out.push_str(&format!("{}{SEPARATOR}{}\n", dep.digest, dep.name));
    }
    out
}

pub(crate) fn decode(text: &str) -> Option<Vec<Dep>> {
    let mut lines = text.lines();
    if lines.next()? != "mockspace-pack-deps-1" {
        return None;
    }
    lines
        .map(|line| {
            let (digest, name) = line.split_once(SEPARATOR)?;
            Some(Dep { name: name.to_string(), digest: digest.to_string() })
        })
        .collect()
}

#[cfg(test)]
#[path = "depinfo_tests.rs"]
mod tests;
