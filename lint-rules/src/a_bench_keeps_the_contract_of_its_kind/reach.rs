//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a bench builds from: every crate its arms reach, followed through
//! their dependencies, and every file of them inside the bench tree. A
//! dependency is inside when it is a `path` into the bench tree, and outside
//! otherwise: a path out of the tree, a git source, a registry crate, a
//! workspace-inherited one. The harness's own crates (`mockspace-bench-*`) are
//! the instrument, not the code measured, and are passed over.
//!
//! Read: `[dependencies]`, `[build-dependencies]` and their `[target.*]` forms,
//! `[patch.*]`, the `[lib] path`, and in Rust source the literal paths of
//! `include!`, `include_str!`, `include_bytes!` and `#[path = "..."]`. Not
//! read: a path built from `env!` or `concat!`, which no lint can resolve.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use mockspace_bench_layout::Kind;

use super::plan::{Bench, rel_str};
use crate::{Entry, View};

/// What one bench builds from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Closure {
    /// Every reach outside the bench tree, said in words.
    pub outside: Vec<String>,
    /// Every file inside the bench tree it builds from, absolute.
    pub files:   BTreeSet<PathBuf>,
}

/// `path` with `.` and `..` taken out lexically.
pub fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                out.pop();
            },
            other => out.push(other),
        }
    }
    out
}

/// What `bench` builds from, its generated arms taking support crates as an
/// arm of `kind` does.
pub fn closure(view: &View<'_>, bench_dir: &Path, bench: &Bench, kind: Kind) -> Closure {
    let tree = normal(bench_dir);
    let root_support: Vec<PathBuf> = view
        .list(bench_dir.join("support"))
        .map(|l| {
            l.entries()
                .iter()
                .filter(|e| Entry::is_dir(e))
                .map(|e| bench_dir.join("support").join(e.name()))
                .collect()
        })
        .unwrap_or_default();
    let mut generated_deps = bench.own_support.clone();
    if kind.generated_arm_takes_root_support() {
        generated_deps.extend(root_support);
    }

    let mut walk = Walk {
        view,
        tree: &tree,
        closure: Closure::default(),
        seen: BTreeSet::new(),
    };
    walk.closure.files.insert(normal(&bench.file));
    let mut queue: Vec<(PathBuf, bool)> = Vec::new();
    for arm in &bench.arms {
        queue.push((normal(&arm.dir), arm.manifest));
        if !arm.manifest {
            queue.extend(generated_deps.iter().map(|d| (normal(d), true)));
        }
    }
    while let Some((dir, manifest)) = queue.pop() {
        if !walk.seen.insert(dir.clone()) {
            continue;
        }
        if !dir.starts_with(&tree) {
            walk.out(format!(
                "builds the crate at {}, outside the bench tree",
                dir.display()
            ));
            continue;
        }
        walk.crate_files(&dir);
        if manifest {
            queue.extend(walk.manifest(&dir).into_iter().map(|d| (d, true)));
        }
    }
    let mut c = walk.closure;
    c.outside.sort();
    c.outside.dedup();
    c
}

struct Walk<'v, 'r> {
    view:    &'v View<'r>,
    tree:    &'v Path,
    closure: Closure,
    seen:    BTreeSet<PathBuf>,
}

impl Walk<'_, '_> {
    fn out(&mut self, why: String) {
        self.closure.outside.push(why);
    }

    fn rel(&self, p: &Path) -> String {
        rel_str(p, self.tree)
    }

    /// Every file of the crate at `dir`, and every file its Rust source names
    /// by a literal path.
    fn crate_files(&mut self, dir: &Path) {
        let skip = |_: &Path, e: &Entry| {
            (Entry::is_dir(e) && (e.name() == "target" || e.name().starts_with('.')))
                || e.name() == "Cargo.lock"
        };
        for f in self.view.walk(dir, skip) {
            let f = normal(&f);
            if f.extension().is_some_and(|x| x == "rs") {
                self.source_paths(&f);
            }
            self.closure.files.insert(f);
        }
    }

    /// The literal paths a Rust file includes or takes a module from.
    fn source_paths(&mut self, file: &Path) {
        let Some(text) = self.view.text(file) else {
            return;
        };
        let base = file.parent().unwrap_or(file);
        let mut found = Vec::new();
        for open in [
            "include!(\"",
            "include_str!(\"",
            "include_bytes!(\"",
            "#[path = \"",
            "#[path=\"",
        ] {
            for (at, _) in text.match_indices(open) {
                let rest = &text[at + open.len() ..];
                if let Some(end) = rest.find('"') {
                    found.push(rest[.. end].to_string());
                }
            }
        }
        for lit in found {
            let target = normal(&base.join(&lit));
            if target.starts_with(self.tree) {
                self.closure.files.insert(target);
            } else {
                let why = format!("{} reads `{lit}`, outside the bench tree", self.rel(file));
                self.out(why);
            }
        }
    }

    /// The in-tree crates the manifest at `dir` depends on, every other
    /// dependency said as a reach outside.
    fn manifest(&mut self, dir: &Path) -> Vec<PathBuf> {
        let file = dir.join("Cargo.toml");
        let Some(Ok(doc)) = self.view.toml(&file) else {
            return Vec::new();
        };
        let who = self.rel(dir);
        let mut tables: Vec<&toml::Table> = Vec::new();
        for key in ["dependencies", "build-dependencies"] {
            tables.extend(doc.get(key).and_then(toml::Value::as_table));
        }
        for t in doc
            .get("target")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flat_map(|t| t.values())
        {
            for key in ["dependencies", "build-dependencies"] {
                tables.extend(t.get(key).and_then(toml::Value::as_table));
            }
        }
        for p in doc
            .get("patch")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flat_map(|t| t.values())
        {
            tables.extend(p.as_table());
        }
        let mut inside = Vec::new();
        for (name, spec) in tables.into_iter().flat_map(|t| t.iter()) {
            let package = spec
                .get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(name);
            if package.starts_with("mockspace-bench-") || name.starts_with("mockspace-bench-") {
                continue;
            }
            match where_from(spec) {
                From::Path(p) => {
                    let target = normal(&dir.join(p));
                    if target.starts_with(self.tree) {
                        inside.push(target);
                    } else {
                        self.out(format!(
                            "`{who}` takes `{name}` by path from `{p}`, outside the bench tree"
                        ));
                    }
                },
                From::Elsewhere(what) => self.out(format!("`{who}` takes `{name}` {what}")),
            }
        }
        if let Some(lib) = doc
            .get("lib")
            .and_then(|l| l.get("path"))
            .and_then(toml::Value::as_str)
        {
            let target = normal(&dir.join(lib));
            if !target.starts_with(dir) {
                if target.starts_with(self.tree) {
                    if let Some(src) = target.parent() {
                        let src = src.to_path_buf();
                        self.crate_files(&src);
                    }
                } else {
                    self.out(format!(
                        "`{who}` builds its library from `{lib}`, outside the bench tree"
                    ));
                }
            }
        }
        inside
    }
}

enum From<'a> {
    Path(&'a str),
    Elsewhere(String),
}

/// Where a dependency spec takes its crate from.
fn where_from(spec: &toml::Value) -> From<'_> {
    if let Some(v) = spec.as_str() {
        return From::Elsewhere(format!("from a registry (`\"{v}\"`)"));
    }
    if let Some(p) = spec.get("path").and_then(toml::Value::as_str) {
        return From::Path(p);
    }
    if let Some(g) = spec.get("git").and_then(toml::Value::as_str) {
        return From::Elsewhere(format!("from git (`{g}`)"));
    }
    if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        return From::Elsewhere("from a workspace it inherits".to_string());
    }
    From::Elsewhere("from a registry".to_string())
}
