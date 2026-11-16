//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Every bench a tree declares, with the kind it states and the crates it
//! measures, read the way the harness loads the tree: the root `bench.toml`'s
//! `[bench.<name>]` sections, and every member `[benchspace]` selects, each one
//! bench (a composed member) or several (a member with sections of its own).

use std::path::{Path, PathBuf};

use mockspace_bench_layout::{Kind, glob_match, lock_name};

use crate::{Entry, View};

/// What a bench's `kind` key says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Declared {
    /// A kind.
    Kind(Kind),
    /// No `kind` key.
    Missing,
    /// A word that names no kind.
    Unknown(String),
}

/// One crate an arm list names: an arm with a manifest of its own, or one
/// whose manifest the tool generates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmRoot {
    /// The crate directory, absolute.
    pub dir:      PathBuf,
    /// Whether the crate carries its own `Cargo.toml`.
    pub manifest: bool,
}

/// One bench, as declared.
#[derive(Clone, Debug)]
pub struct Bench {
    /// The bench key the harness gives it: the member, `<member>/<section>`, or
    /// a root section's name.
    pub key:         String,
    /// The `bench.toml` declaring it, absolute.
    pub file:        PathBuf,
    /// The section, for a bench declared as `[bench.<section>]`.
    pub section:     Option<String>,
    /// Its `kind`.
    pub declared:    Declared,
    /// Where its lock sits, absolute, whatever its kind.
    pub lock:        PathBuf,
    /// The crates its arms are.
    pub arms:        Vec<ArmRoot>,
    /// Its own `support/` crates (a composed member's), absolute.
    pub own_support: Vec<PathBuf>,
    /// Every workload program it and its sweeps name.
    pub workloads:   Vec<String>,
}

impl Bench {
    /// Whether it is a `[bench.<name>]` section of the root `bench.toml`,
    /// whose file every other root section and the tree's setup share, so
    /// only its own section of it is the bench's.
    #[must_use]
    pub fn is_root_section(&self, bench_dir: &Path) -> bool {
        self.section.is_some()
            && super::reach::normal(&self.file) == super::reach::normal(&bench_dir.join("bench.toml"))
    }
}

/// The workloads a bench or sweep table names, `default` where a composed
/// bench names none.
fn workloads_of(tables: &[&toml::Value], default: bool) -> Vec<String> {
    let mut out: Vec<String> = tables
        .iter()
        .filter_map(|t| t.get("workload").and_then(toml::Value::as_str).map(str::to_string))
        .collect();
    if default && tables.first().is_some_and(|t| t.get("workload").is_none()) {
        out.push("default".to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// Every bench under `bench_dir`, in key order. Empty where the tree has no
/// root `bench.toml`.
pub fn benches(view: &View<'_>, bench_dir: &Path) -> Vec<Bench> {
    let root_file = bench_dir.join("bench.toml");
    let Some(Ok(root)) = view.toml(&root_file) else {
        return Vec::new();
    };
    let mut out = sections(root, &root_file, bench_dir, None);
    for member in members(view, bench_dir, root) {
        let dir = bench_dir.join(&member);
        let file = dir.join("bench.toml");
        let Some(Ok(doc)) = view.toml(&file) else {
            continue;
        };
        if doc.contains_key("bench") {
            out.extend(sections(doc, &file, &dir, Some(&member)));
        } else {
            out.push(composed(view, doc, &file, &dir, &member));
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The member directories `[benchspace]` selects, relative to `bench_dir`. A
/// directory with a `bench.toml` that a members pattern matches is a member and
/// is not descended into; any other directory is descended into.
fn members(view: &View<'_>, bench_dir: &Path, root: &toml::Table) -> Vec<String> {
    let list = |key: &str| -> Option<Vec<String>> {
        let arr = root.get("benchspace")?.get(key)?.as_array()?;
        Some(
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        )
    };
    let patterns = list("members").unwrap_or_else(|| vec!["**".to_string()]);
    let exclude = list("exclude").unwrap_or_default();
    let mut found = Vec::new();
    let mut stack = vec![bench_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Some(listing) = view.list(&dir) else {
            continue;
        };
        for entry in listing.entries() {
            let name = entry.name();
            if !Entry::is_dir(entry) || name.starts_with('.') || name == "target" {
                continue;
            }
            let path = dir.join(name);
            let rel = rel_str(&path, bench_dir);
            let matched = patterns.iter().any(|p| glob_match(p, &rel));
            if matched && view.is_file(path.join("bench.toml")) {
                if !exclude.iter().any(|p| glob_match(p, &rel)) {
                    found.push(rel);
                }
                continue;
            }
            stack.push(path);
        }
    }
    found.sort();
    found
}

/// The benches of a file declaring `[bench.<name>]` sections, rooted at `base`.
fn sections(doc: &toml::Table, file: &Path, base: &Path, member: Option<&str>) -> Vec<Bench> {
    let Some(table) = doc.get("bench").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    table
        .iter()
        .map(|(name, section)| {
            let entries = arm_entries(section);
            Bench {
                key:         member.map_or_else(|| name.clone(), |m| format!("{m}/{name}")),
                file:        file.to_path_buf(),
                section:     Some(name.clone()),
                declared:    declared(section),
                lock:        base.join(lock_name(Some(name))),
                arms:        entries
                    .iter()
                    .map(|e| {
                        ArmRoot {
                            dir:      entry_crate(base, e),
                            manifest: true,
                        }
                    })
                    .collect(),
                own_support: Vec::new(),
                workloads:   workloads_of(&[section], false),
            }
        })
        .collect()
}

/// A composed member: one bench, its arms the directories under `arms/` and
/// any arm a list names by path.
fn composed(view: &View<'_>, doc: &toml::Table, file: &Path, dir: &Path, member: &str) -> Bench {
    let mut arms: Vec<ArmRoot> = view
        .list(dir.join("arms"))
        .map(|l| {
            l.entries()
                .iter()
                .filter(|e| Entry::is_dir(e))
                .map(|e| {
                    let d = dir.join("arms").join(e.name());
                    ArmRoot {
                        manifest: view.is_file(d.join("Cargo.toml")),
                        dir:      d,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let mut lists: Vec<toml::Value> = vec![toml::Value::Table(doc.clone())];
    if let Some(sweeps) = doc.get("sweep").and_then(toml::Value::as_table) {
        lists.extend(sweeps.values().cloned());
    }
    for e in lists
        .iter()
        .flat_map(arm_entries)
        .filter(|e| e.contains('/'))
    {
        arms.push(ArmRoot {
            dir:      entry_crate(dir, &e),
            manifest: true,
        });
    }
    let own_support = view
        .list(dir.join("support"))
        .map(|l| {
            l.entries()
                .iter()
                .filter(|e| Entry::is_dir(e))
                .map(|e| dir.join("support").join(e.name()))
                .collect()
        })
        .unwrap_or_default();
    Bench {
        key: member.to_string(),
        file: file.to_path_buf(),
        section: None,
        declared: declared(&toml::Value::Table(doc.clone())),
        lock: dir.join(lock_name(None)),
        arms,
        own_support,
        workloads: workloads_of(&lists.iter().collect::<Vec<_>>(), true),
    }
}

fn declared(v: &toml::Value) -> Declared {
    match v.get("kind") {
        None => Declared::Missing,
        Some(k) => {
            match k.as_str() {
                Some(s) => {
                    Kind::parse(s)
                        .map_or_else(|| Declared::Unknown(format!("\"{s}\"")), Declared::Kind)
                },
                None => Declared::Unknown(format!("<{}>", k.type_str())),
            }
        },
    }
}

/// Every arm entry a bench or sweep table names: its `arms` or `variants`, and
/// each point's own.
fn arm_entries(v: &toml::Value) -> Vec<String> {
    let strings = |v: Option<&toml::Value>| -> Vec<String> {
        v.and_then(toml::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = strings(v.get("arms"));
    out.extend(strings(v.get("variants")));
    for key in ["points", "sizes"] {
        for p in v
            .get(key)
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            out.extend(strings(p.get("arms")));
            out.extend(strings(p.get("variants")));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The crate an entry builds from: a short name is `variants/<name>/` under
/// `base`, a path is its components up to the `target` directory it points
/// into.
fn entry_crate(base: &Path, entry: &str) -> PathBuf {
    if !entry.contains('/') {
        return base.join("variants").join(entry);
    }
    let kept: PathBuf = Path::new(entry)
        .components()
        .take_while(|c| c.as_os_str() != "target")
        .collect();
    super::reach::normal(&base.join(kept))
}

/// `path` relative to `base`, `/`-separated.
pub fn rel_str(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
