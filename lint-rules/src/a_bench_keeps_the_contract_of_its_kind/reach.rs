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
//! [`arm_closure`] follows one arm out of the tree as well, through every
//! crate it takes by path wherever it sits, for the digest of a tracking
//! arm's source. A crate taken through a workspace it inherits is not
//! followed here, since resolving one is cargo's: the tool asks cargo for
//! every path package an arm's build resolved and adds them through
//! [`crates_as_built`].
//!
//! Read: `[dependencies]`, `[build-dependencies]` and their `[target.*]` forms,
//! `[patch.*]`, the `[lib] path`, and in Rust source the literal paths of
//! `include!`, `include_str!`, `include_bytes!` and `#[path = "..."]`. Not
//! read: a path built from `env!` or `concat!`, which no lint can resolve.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use mockspace_bench_layout::Kind;

use super::plan::{ArmRoot, Bench, rel_str};
use crate::{Entry, View};

/// What a bench, or one arm, builds from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Closure {
    /// Every reach outside the bench tree, said in words.
    pub outside: Vec<String>,
    /// Every file it builds from that was read, absolute: inside the bench
    /// tree only, unless the walk followed crates out of it.
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

/// `path` relative to `base`, both taken lexically, climbing out of `base`
/// with `..` where `path` is not under it: the same spelling in every
/// checkout, wherever it sits on the disk.
pub fn rel_up(path: &Path, base: &Path) -> String {
    let (path, base) = (normal(path), normal(base));
    let p: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = p.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_string(); b.len() - common];
    parts.extend(
        p[common ..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// What `bench` builds from inside the bench tree, and every reach out of it,
/// its generated arms taking support crates as an arm of `kind` does.
pub fn closure(view: &View<'_>, bench_dir: &Path, bench: &Bench, kind: Kind) -> Closure {
    let mut starts = Vec::new();
    for arm in &bench.arms {
        starts.extend(arm_starts(view, bench_dir, bench, arm, kind));
    }
    let mut c = walk_from(view, bench_dir, starts, false);
    own_file(&mut c, bench_dir, bench);
    c
}

/// The bench's own `bench.toml` into `c`, where the file is its own. A root
/// section shares the root file, and its own section of it is read with the
/// setup instead.
fn own_file(c: &mut Closure, bench_dir: &Path, bench: &Bench) {
    if !bench.is_root_section(bench_dir) {
        c.files.insert(normal(&bench.file));
    }
}

/// The files of the crates at `dirs` as they are built, wherever they sit,
/// and of every crate they take by path: in the bench tree every file under
/// the crate, out of it the manifest, the build script and the library's
/// source, with the files that source includes by a literal path.
pub fn crates_as_built(view: &View<'_>, bench_dir: &Path, dirs: &[PathBuf]) -> BTreeSet<PathBuf> {
    walk_from(
        view,
        bench_dir,
        dirs.iter().map(|d| (normal(d), true)).collect(),
        true,
    )
    .files
}

/// Every file one arm builds from, in the bench tree and out of it: the
/// crates it reaches by path wherever they are, followed through their own
/// path dependencies, the files their source includes, and the bench's
/// `bench.toml`. Out of the tree a crate is read as cargo builds it: its
/// manifest, its build script and its library's source directory.
pub fn arm_closure(
    view: &View<'_>,
    bench_dir: &Path,
    bench: &Bench,
    arm: &ArmRoot,
    kind: Kind,
) -> Closure {
    let mut c = walk_from(
        view,
        bench_dir,
        arm_starts(view, bench_dir, bench, arm, kind),
        true,
    );
    own_file(&mut c, bench_dir, bench);
    c
}

/// Where a walk of one arm starts: its crate, and for a generated manifest the
/// support crates its kind takes.
fn arm_starts(
    view: &View<'_>,
    bench_dir: &Path,
    bench: &Bench,
    arm: &ArmRoot,
    kind: Kind,
) -> Vec<(PathBuf, bool)> {
    let mut starts = vec![(normal(&arm.dir), arm.manifest)];
    if !arm.manifest {
        starts.extend(bench.own_support.iter().map(|d| (normal(d), true)));
        if kind.generated_arm_takes_root_support() {
            let support = bench_dir.join("support");
            let roots: Vec<PathBuf> = view
                .list(&support)
                .map(|l| {
                    l.entries()
                        .iter()
                        .filter(|e| Entry::is_dir(e))
                        .map(|e| support.join(e.name()))
                        .collect()
                })
                .unwrap_or_default();
            starts.extend(roots.into_iter().map(|d| (normal(&d), true)));
        }
    }
    starts
}

/// Walk the crates from `starts`, each `(dir, has a manifest)`. With
/// `follow`, a crate out of the tree is read and followed too; without, it is
/// only said as a reach outside.
fn walk_from(
    view: &View<'_>,
    bench_dir: &Path,
    starts: Vec<(PathBuf, bool)>,
    follow: bool,
) -> Closure {
    let tree = normal(bench_dir);
    let mut walk = Walk {
        view,
        tree: &tree,
        closure: Closure::default(),
        seen: BTreeSet::new(),
        follow,
    };
    let mut queue = starts;
    while let Some((dir, manifest)) = queue.pop() {
        if !walk.seen.insert(dir.clone()) {
            continue;
        }
        if dir.starts_with(&tree) {
            walk.files_under(&dir);
        } else if follow {
            walk.files_as_built(&dir);
        } else {
            walk.out(format!(
                "builds the crate at {}, outside the bench tree",
                dir.display()
            ));
            continue;
        }
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
    follow:  bool,
}

impl Walk<'_, '_> {
    fn out(&mut self, why: String) {
        self.closure.outside.push(why);
    }

    fn rel(&self, p: &Path) -> String {
        rel_str(p, self.tree)
    }

    /// Every file under `dir`, and every file its Rust source names by a
    /// literal path.
    fn files_under(&mut self, dir: &Path) {
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

    /// A crate out of the tree, as cargo builds it: its manifest, its build
    /// script, and `src/`. A library elsewhere is read where the manifest
    /// names it. Its tests, benches and examples are not built into an arm.
    fn files_as_built(&mut self, dir: &Path) {
        for f in ["Cargo.toml", "build.rs"] {
            if self.view.is_file(dir.join(f)) {
                self.closure.files.insert(normal(&dir.join(f)));
            }
        }
        if self.view.is_dir(dir.join("src")) {
            self.files_under(&dir.join("src"));
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
            if !target.starts_with(self.tree) {
                let why = format!("{} reads `{lit}`, outside the bench tree", self.rel(file));
                self.out(why);
                if !self.follow {
                    continue;
                }
            }
            self.closure.files.insert(target);
        }
    }

    /// The crates the manifest at `dir` takes by path that the walk goes on
    /// into: those in the tree, and with `follow` every one. Every other
    /// dependency is said as a reach outside.
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
        let mut next = Vec::new();
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
                    if !target.starts_with(self.tree) {
                        self.out(format!(
                            "`{who}` takes `{name}` by path from `{p}`, outside the bench tree"
                        ));
                        if !self.follow {
                            continue;
                        }
                    }
                    next.push(target);
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
                if !target.starts_with(self.tree) {
                    self.out(format!(
                        "`{who}` builds its library from `{lib}`, outside the bench tree"
                    ));
                }
                if target.starts_with(self.tree) || self.follow {
                    if let Some(src) = target.parent() {
                        let src = src.to_path_buf();
                        self.files_under(&src);
                    }
                }
            }
        }
        next
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
