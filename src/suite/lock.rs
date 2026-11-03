//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a lockfile pins for one package, rather than for the whole workspace.
//!
//! A member's tests are built from the packages it reaches through the lock,
//! and from no others. Salting every fingerprint with the whole file made a
//! lock move that touched one edge of the graph (a version bumped for one
//! member) rerun every member. The digest here takes the member's own entry and
//! the entries of everything it reaches, all dependency kinds followed, so it
//! moves when something the member is built from moved and when nothing else
//! did, within the limit stated below. A dev-dependency of a dependency is
//! followed too, which makes the
//! closure wider than the build needs and never narrower.
//!
//! **What the lock cannot say** is which cargo features the build unifies. A
//! bump in one member's dependency that turns on a feature of a crate another
//! member also builds moves the first member and not the second, whose closure
//! is unchanged. Feature edits in a manifest had the same blind spot before:
//! manifests are fingerprinted per member, and only as that member's own input.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name:     String,
    version:  String,
    source:   String,
    checksum: String,
    deps:     Vec<String>,
}

/// A parsed `Cargo.lock`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lock {
    entries: Vec<Entry>,
    by_name: BTreeMap<String, Vec<usize>>,
}

impl Lock {
    /// `None` when the text is not a lockfile this can read, in which case the
    /// caller keeps salting with the file whole.
    pub fn parse(text: &str) -> Option<Lock> {
        let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        let packages = doc.get("package")?.as_array_of_tables()?;
        let mut lock = Lock::default();
        for p in packages {
            let text = |key: &str| {
                p.get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let mut deps: Vec<String> = p
                .get("dependencies")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            deps.sort();
            let entry = Entry {
                name: text("name"),
                version: text("version"),
                source: text("source"),
                checksum: text("checksum"),
                deps,
            };
            if entry.name.is_empty() {
                return None;
            }
            lock.by_name
                .entry(entry.name.clone())
                .or_default()
                .push(lock.entries.len());
            lock.entries.push(entry);
        }
        Some(lock)
    }

    /// The digest of `member`'s entry and everything reachable from it, or
    /// `None` when the lock has no entry for it.
    pub fn closure_digest(&self, member: &str) -> Option<[u8; 32]> {
        let start = self.by_name.get(member)?;
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut stack: Vec<usize> = start.clone();
        while let Some(i) = stack.pop() {
            if !seen.insert(i) {
                continue;
            }
            for dep in &self.entries[i].deps {
                stack.extend(self.resolve(dep));
            }
        }
        let mut h = Sha256::new();
        // The member's own entry first and named, so two members with the same
        // closure under different names still differ.
        h.update(format!("lock-closure\0{member}\0").as_bytes());
        for i in seen {
            let e = &self.entries[i];
            for part in [&e.name, &e.version, &e.source, &e.checksum] {
                h.update(part.as_bytes());
                h.update([0]);
            }
            for d in &e.deps {
                h.update(d.as_bytes());
                h.update([1]);
            }
            h.update([2]);
        }
        Some(h.finalize().into())
    }

    /// The entries a dependency string names: `name`, `name version` or
    /// `name version (source)`. A name alone, which a lock writes when only one
    /// version of it exists, takes every entry of that name.
    fn resolve(&self, dep: &str) -> Vec<usize> {
        let mut words = dep.split_whitespace();
        let Some(name) = words.next() else {
            return Vec::new();
        };
        let version = words.next();
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|i| version.is_none_or(|v| self.entries[*i].version == v))
            .collect()
    }
}

/// What every digest of a member starts from: the workspace's inputs, the
/// generated cargo config, and the lock entries the member is built from.
///
/// A lockfile that reads is held apart and salted per member by its closure;
/// one that does not read is salted whole, into everyone, as every other
/// workspace input is.
#[derive(Clone)]
pub struct Salt {
    base:  Sha256,
    locks: Vec<(String, Lock, [u8; 32])>,
}

impl Salt {
    pub fn new(scheme: &str) -> Salt {
        let mut base = Sha256::new();
        base.update(format!("scheme\0{scheme}\0").as_bytes());
        Salt {
            base,
            locks: Vec::new(),
        }
    }

    /// One workspace input, named by where it sits relative to the repository
    /// (never by absolute path, so the same tree on another machine has the same
    /// fingerprints), with its content digest and, for a lockfile, a reader of
    /// its text.
    pub fn input(&mut self, label: &str, whole: [u8; 32], lock_text: Option<String>) {
        if let Some(lock) = lock_text.as_deref().and_then(Lock::parse) {
            self.locks.push((label.to_string(), lock, whole));
        } else {
            self.base.update(label.as_bytes());
            self.base.update(whole);
        }
    }

    pub fn cargo_config(&mut self, text: &str) {
        self.base.update(b"cargo-config\0");
        self.base.update(text.as_bytes());
    }

    /// The salt for one member.
    pub fn of(&self, member: &str) -> [u8; 32] {
        let mut h = self.base.clone();
        for (label, lock, whole) in &self.locks {
            h.update(label.as_bytes());
            h.update(lock.closure_digest(member).unwrap_or(*whole));
        }
        h.finalize().into()
    }
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
