//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A bench declares its kind and keeps that kind's contract.
//!
//! Every bench is a snapshot or a tracking bench ([`Kind`]), and says which in
//! its `bench.toml`. A snapshot builds only from inside the bench tree, and its
//! lock records every file it builds from, so the reference it is never
//! changes under its history. A tracking bench builds against code outside the
//! tree, which is what its history across commits follows. Found and refused:
//!
//! - a bench that declares no kind, or a word that names none;
//! - a snapshot whose arms reach anything outside the bench tree;
//! - a snapshot with no lock, or whose files are no longer the ones its lock
//!   records;
//! - a tracking bench whose arms reach nothing outside the bench tree, which
//!   is a snapshot without the lock that would keep it one;
//! - a lock beside a bench that is not a snapshot.
//!
//! `mock bench run` asks [`bench_kinds`] before it builds anything, so the tool
//! refuses what the gate refuses; `mock bench freeze` writes a lock from
//! [`lock_body`], and `mock bench migrate --kinds` writes the kind each bench's
//! reach admits from [`plans`].

mod plan;
mod reach;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mockspace_bench_layout::{Kind, LOCK_HEADER};
use sha2::{Digest, Sha256};

pub use self::plan::Declared;
use self::plan::rel_str;
use crate::{Lint, LintError, RepoContext, RepoLint, Severity, View};

const NAME: &str = "a-bench-keeps-the-contract-of-its-kind";

/// One bench as the tool sees it: what it declares, what its reach admits,
/// and the lock its files would write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindPlan {
    /// The bench key.
    pub bench:     String,
    /// The `bench.toml` declaring it, relative to the bench tree.
    pub file:      PathBuf,
    /// The `[bench.<section>]` it is, where it is one.
    pub section:   Option<String>,
    /// What its `kind` key says.
    pub declared:  Declared,
    /// The only kind its reach admits: snapshot where it builds from nothing
    /// outside the bench tree as a snapshot's arms would, tracking otherwise.
    pub admits:    Kind,
    /// Every reach outside the bench tree under the declared kind, or under
    /// the snapshot reading where none is declared.
    pub outside:   Vec<String>,
    /// Where its lock sits, relative to the bench tree.
    pub lock:      PathBuf,
    /// The lock its files would write today, where it builds as a snapshot.
    pub lock_body: Option<String>,
}

/// Every bench in the tree at `bench_dir`, read through `view`.
#[must_use]
pub fn plans(view: &View<'_>, bench_dir: &Path) -> Vec<KindPlan> {
    plan::benches(view, bench_dir)
        .into_iter()
        .map(|b| {
            let snapshot = reach::closure(view, bench_dir, &b, Kind::Snapshot);
            let admits = if snapshot.outside.is_empty() { Kind::Snapshot } else { Kind::Tracking };
            let read_as = match &b.declared {
                Declared::Kind(k) => *k,
                _ => Kind::Snapshot,
            };
            let closure = if read_as == Kind::Snapshot {
                snapshot
            } else {
                reach::closure(view, bench_dir, &b, read_as)
            };
            KindPlan {
                bench: b.key.clone(),
                file: PathBuf::from(rel_str(&b.file, bench_dir)),
                section: b.section.clone(),
                declared: b.declared.clone(),
                admits,
                lock_body: (admits == Kind::Snapshot)
                    .then(|| lock_body(view, bench_dir, &closure.files)),
                outside: closure.outside,
                lock: PathBuf::from(rel_str(&b.lock, bench_dir)),
            }
        })
        .collect()
}

/// The lock of `files`: a header, then `<sha256>  <path>` per file, the path
/// relative to the bench tree, in path order.
fn lock_body(
    view: &View<'_>,
    bench_dir: &Path,
    files: &std::collections::BTreeSet<PathBuf>,
) -> String {
    let tree = reach::normal(bench_dir);
    let mut lines: Vec<(String, String)> = files
        .iter()
        .map(|f| {
            let hex: String = view
                .bytes(f)
                .map(|b| {
                    Sha256::digest(b)
                        .iter()
                        .map(|x| format!("{x:02x}"))
                        .collect()
                })
                .unwrap_or_else(|| "missing".to_string());
            (rel_str(f, &tree), hex)
        })
        .collect();
    lines.sort();
    let mut out = format!("{LOCK_HEADER}\n");
    for (path, hex) in lines {
        out.push_str(&format!("{hex}  {path}\n"));
    }
    out
}

/// A lock's rows, path to digest.
fn lock_rows(text: &str) -> BTreeMap<&str, &str> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once("  ").map(|(h, p)| (p, h)))
        .collect()
}

/// How the lock `was` differs from `now`, one line per file.
fn lock_changes(was: &str, now: &str) -> Vec<String> {
    let (was, now) = (lock_rows(was), lock_rows(now));
    let mut out = Vec::new();
    for (path, hex) in &now {
        match was.get(path) {
            None => out.push(format!("{path} is new")),
            Some(h) if h != hex => out.push(format!("{path} changed")),
            Some(_) => {},
        }
    }
    out.extend(
        was.keys()
            .filter(|p| !now.contains_key(*p))
            .map(|p| format!("{p} is gone")),
    );
    out
}

/// One bench out of its kind's contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindFinding {
    /// The bench key.
    pub bench:   String,
    /// The file to act on, relative to the bench tree.
    pub path:    PathBuf,
    /// What is wrong and what to do.
    pub message: String,
}

impl std::fmt::Display for KindFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

/// Every bench in the tree at `bench_dir` out of its kind's contract. Empty
/// where the tree does not exist. `mock bench run` and the lint ask this one
/// predicate.
#[must_use]
pub fn bench_kinds(view: &View<'_>, bench_dir: &Path) -> Vec<KindFinding> {
    let mut out = Vec::new();
    for p in plans(view, bench_dir) {
        let lock_abs = bench_dir.join(&p.lock);
        let mut say = |path: &Path, message: String| {
            out.push(KindFinding {
                bench: p.bench.clone(),
                path: path.to_path_buf(),
                message,
            });
        };
        let kind = match &p.declared {
            Declared::Missing => {
                let msg = format!(
                    "bench `{}` declares no `kind`. Its arms admit `kind = \"{}\"`; {}. Declare it, \
                     or run `mock bench migrate --kinds`, which writes it.",
                    p.bench,
                    p.admits,
                    p.admits.contract()
                );
                say(&p.file, msg);
                continue;
            },
            Declared::Unknown(word) => {
                let msg = format!(
                    "bench `{}` declares `kind = {word}`, which names no kind. A bench is \
                     `\"snapshot\"` or `\"tracking\"`.",
                    p.bench
                );
                say(&p.file, msg);
                continue;
            },
            Declared::Kind(k) => *k,
        };
        match kind {
            Kind::Snapshot if !p.outside.is_empty() => {
                let msg = format!(
                    "snapshot `{}` reaches outside the bench tree: {}. {}. Bring what it builds \
                     from into the bench, or declare it `kind = \"tracking\"` if following living \
                     code is what it is for.",
                    p.bench,
                    p.outside.join("; "),
                    Kind::Snapshot.contract()
                );
                say(&p.file, msg);
            },
            Kind::Snapshot => {
                let now = p.lock_body.as_deref().unwrap_or_default();
                match view.text(&lock_abs) {
                    None => {
                        say(
                            &p.lock,
                            format!(
                                "snapshot `{}` has no lock. Run `mock bench freeze {}`, which writes \
                             it, and commit it with the bench.",
                                p.bench, p.bench
                            ),
                        );
                    },
                    Some(was) => {
                        let changes = lock_changes(was, now);
                        if !changes.is_empty() {
                            say(
                                &p.lock,
                                format!(
                                    "snapshot `{}` is no longer the code its lock froze: {}. A snapshot \
                                 never changes: put the files back, or make the changed code a new \
                                 bench beside it and keep this one as the reference.",
                                    p.bench,
                                    changes.join(", ")
                                ),
                            );
                        }
                    },
                }
            },
            Kind::Tracking => {
                if p.outside.is_empty() {
                    say(
                        &p.file,
                        format!(
                            "tracking bench `{}` builds only from inside the bench tree, so nothing \
                         its history follows can change. {}. Point an arm at the living code it \
                         is meant to follow, or declare it `kind = \"snapshot\"`.",
                            p.bench,
                            Kind::Tracking.contract()
                        ),
                    );
                }
                if view.is_file(&lock_abs) {
                    say(
                        &p.lock,
                        format!(
                            "a snapshot lock sits beside tracking bench `{}`, which has none. Delete \
                         it.",
                            p.bench
                        ),
                    );
                }
            },
        }
    }
    out
}

/// The lint.
pub struct ABenchKeepsTheContractOfItsKind;

impl Lint for ABenchKeepsTheContractOfItsKind {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "every bench declares `kind = \"snapshot\"` or `\"tracking\"` and keeps that kind's contract: \
         a snapshot builds only from the bench tree and matches its lock, a tracking bench reaches \
         living code"
    }

    fn source_only(&self) -> bool {
        false
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl RepoLint for ABenchKeepsTheContractOfItsKind {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let bench_dir = ctx.mock_dir.join("benches");
        let tree = bench_dir
            .strip_prefix(ctx.repo_root)
            .unwrap_or(&bench_dir)
            .display()
            .to_string();
        bench_kinds(ctx.view(), &bench_dir)
            .into_iter()
            .map(|f| {
                let mut e = LintError::error(tree.clone(), 0, NAME, f.message);
                e.path = Some(f.path.display().to_string());
                e
            })
            .collect()
    }
}
