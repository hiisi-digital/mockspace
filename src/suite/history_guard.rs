//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit gate over the test-history files: a green record is not nulled by
//! a commit that changed nothing the member's tests are built from.
//!
//! A run cut short (a container restart, a full disk) can leave a member's file
//! without its green record, and the next commit carries that loss into the
//! repository, where it costs every clone one full run of that member. Nothing
//! about the commit explains it: no source of the member moved, so no run was
//! owed. `mock check-test-history`, which the pre-commit hook runs when a
//! test-history file is staged, refuses it and names the member.
//!
//! **A green nulled by a commit that touches the member's inputs is the record
//! doing its job**: the member's suite failed there, or the run that would
//! re-record it is the next one. Its inputs are everything its fingerprint is
//! made of (see [`super::fingerprint`]): its own directory, the directories of
//! every member compiled into its tests, the paths its sources read from
//! outside, and the workspace-wide files every build reads.
//!
//! **A merge commit is told apart by what the merge driver would have written.**
//! The driver drops a green where both sides moved it to different values
//! (see [`super::history_merge`]), and that null is not a loss. A null in a merge
//! commit is accepted when the same three-way rule over the base, the first
//! parent and the merged-in parent gives none, or when the merge changed the
//! member's inputs as any commit may; a green the rule would have kept that the
//! merge commit nulls is the cut-short run again.
//!
//! **A squash merge is not told apart, on purpose.** `git merge --squash` runs
//! the driver but leaves no `MERGE_HEAD`, so the commit that follows is an
//! ordinary one to this gate. Its null green is accepted when the squashed
//! changes touch the member's inputs, as for any commit. When they do not, the
//! tree it lands is the first parent's own for that member, whose recorded green
//! still describes it, so the null is a loss and the refusal is right; unstaging
//! the file keeps that green.
//!
//! The paths a commit changed are read from the index against `HEAD`, so what is
//! checked is what the commit will hold and not the working tree. A member the
//! workspace no longer has is not guarded, since its record going is a removal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use super::fingerprint::WORKSPACE_INPUTS;
use super::graph::Graph;
use super::history::{Green, MemberFile};
use super::history_merge::merged_green;

/// A member's green record in a commit that removes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Nulled {
    /// The file, relative to the repository root.
    pub path:   String,
    pub member: String,
    /// The first parent's, the merged-in parent's and their merge base's green
    /// record for it, when the commit concludes a merge.
    pub merge:  Option<Sides>,
}

/// Three versions of one member's green record.
#[derive(Debug, Clone, PartialEq)]
pub struct Sides {
    pub base:   Option<Green>,
    pub ours:   Option<Green>,
    pub theirs: Option<Green>,
}

/// What decides whether a commit touched a member's inputs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inputs {
    /// Each member's directory, relative to the repository root. A member
    /// outside the repository has none, and no commit of it touches one.
    pub dirs:      BTreeMap<String, PathBuf>,
    /// The members compiled into each member's tests.
    pub closure:   BTreeMap<String, BTreeSet<String>>,
    /// What each member's sources read from outside their directory.
    pub reaches:   BTreeMap<String, Vec<PathBuf>>,
    /// The files every build reads.
    pub workspace: Vec<PathBuf>,
}

impl Inputs {
    /// Read from the workspace: its graph, and what the sources of each member
    /// reach.
    pub fn read(repo_root: &Path, mock_dir: &Path) -> Result<Inputs, String> {
        let graph = Graph::read(mock_dir)?;
        let fp = super::fingerprint::compute(repo_root, mock_dir, &graph, None)?;
        let mut inputs = Inputs::default();
        for (name, pkg) in &graph.packages {
            if let Ok(dir) = pkg.dir.strip_prefix(repo_root) {
                inputs.dirs.insert(name.clone(), dir.to_path_buf());
            }
            inputs
                .closure
                .insert(name.clone(), graph.test_closure(name));
            inputs.reaches.insert(
                name.clone(),
                fp.reaches.get(name).cloned().unwrap_or_default(),
            );
        }
        let mock_rel = mock_dir.strip_prefix(repo_root).unwrap_or(Path::new(""));
        inputs.workspace = [mock_rel, Path::new("")]
            .iter()
            .flat_map(|base| WORKSPACE_INPUTS.iter().map(move |n| base.join(n)))
            .collect();
        Ok(inputs)
    }

    /// Whether any of `changed` is something `member`'s fingerprint is made of.
    /// A member this does not know is touched by anything: it has gone, and its
    /// record going with it is not a loss.
    pub fn touches(&self, member: &str, changed: &[String]) -> bool {
        if !self.closure.contains_key(member) {
            return true;
        }
        let own_and_closure = std::iter::once(member.to_string())
            .chain(self.closure[member].iter().cloned())
            .collect::<Vec<_>>();
        changed.iter().map(Path::new).any(|c| {
            self.workspace.iter().any(|w| c == w)
                || own_and_closure.iter().any(|m| {
                    self.dirs.get(m).is_some_and(|d| c.starts_with(d))
                        || self
                            .reaches
                            .get(m)
                            .is_some_and(|rs| rs.iter().any(|r| c.starts_with(r)))
                })
        })
    }
}

/// Why a null green is refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub path:   String,
    pub member: String,
    pub why:    &'static str,
}

const UNTOUCHED: &str = "the commit changes nothing its tests are built from";
const KEPT_BY_THE_DRIVER: &str = "both sides of the merge record the same green, or only one moved it, and the merge commit \
     nulls it";

/// The refusals among `nulled`, given the paths the commit changed. `inputs` is
/// asked only when a null is not already explained by the merge.
pub fn judge(
    nulled: &[Nulled],
    changed: &[String],
    inputs: &mut dyn FnMut() -> Result<Inputs, String>,
) -> Result<Vec<Refusal>, String> {
    let mut asked: Option<Inputs> = None;
    let mut out = Vec::new();
    for n in nulled {
        if let Some(s) = &n.merge {
            if merged_green(&s.base, &s.ours, &s.theirs).0.is_none() {
                continue;
            }
        }
        if asked.is_none() {
            asked = Some(inputs()?);
        }
        let touched = asked
            .as_ref()
            .is_some_and(|i| i.touches(&n.member, changed));
        if !touched {
            out.push(Refusal {
                path:   n.path.clone(),
                member: n.member.clone(),
                why:    if n.merge.is_some() { KEPT_BY_THE_DRIVER } else { UNTOUCHED },
            });
        }
    }
    Ok(out)
}

fn git(repo_root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn blob(repo_root: &Path, spec: &str) -> Option<String> {
    git(repo_root, &["show", spec]).and_then(|b| String::from_utf8(b).ok())
}

fn green_at(repo_root: &Path, spec: &str) -> Option<Green> {
    blob(repo_root, spec)
        .and_then(|t| MemberFile::parse(&t).ok())
        .and_then(|m| m.green)
}

fn lines(bytes: Vec<u8>) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// Every member whose green record the staged commit removes, from the index
/// against `HEAD`. A repository with no commit yet has none.
pub fn staged_nulls(repo_root: &Path, history_dir: &str) -> Vec<Nulled> {
    if git(repo_root, &["rev-parse", "-q", "--verify", "HEAD"]).is_none() {
        return Vec::new();
    }
    let Some(listing) = git(repo_root, &[
        "diff",
        "--cached",
        "--name-only",
        "-z",
        "--no-renames",
        "--diff-filter=MD",
        "HEAD",
        "--",
        history_dir,
    ]) else {
        return Vec::new();
    };
    let merging = git(repo_root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_some();
    let base = merging
        .then(|| git(repo_root, &["merge-base", "HEAD", "MERGE_HEAD"]))
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|b| !b.is_empty());
    let mut out = Vec::new();
    for path in lines(listing).into_iter().filter(|p| p.ends_with(".json")) {
        let Some(before) = green_at(repo_root, &format!("HEAD:{path}")) else {
            continue;
        };
        let after = blob(repo_root, &format!(":{path}"))
            .and_then(|t| MemberFile::parse(&t).ok())
            .and_then(|m| m.green);
        if after.is_some() {
            continue;
        }
        let member = Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let merge = merging.then(|| {
            Sides {
                base:   base
                    .as_ref()
                    .and_then(|b| green_at(repo_root, &format!("{b}:{path}"))),
                ours:   Some(before),
                theirs: green_at(repo_root, &format!("MERGE_HEAD:{path}")),
            }
        });
        out.push(Nulled {
            path,
            member,
            merge,
        });
    }
    out
}

/// `mock check-test-history`: exit 0 when the staged commit nulls no green record
/// it should not, 1 when it does, naming each member.
pub fn run(repo_root: &Path, mock_dir: &Path) -> ExitCode {
    let rel = mock_dir.strip_prefix(repo_root).unwrap_or(Path::new(""));
    let history_dir = rel
        .join("test-history")
        .to_string_lossy()
        .replace('\\', "/");
    let nulled = staged_nulls(repo_root, &history_dir);
    if nulled.is_empty() {
        return ExitCode::SUCCESS;
    }
    let changed = git(repo_root, &[
        "diff",
        "--cached",
        "--name-only",
        "-z",
        "HEAD",
    ])
    .map(lines)
    .unwrap_or_default()
    .into_iter()
    .filter(|p| !p.starts_with(&format!("{history_dir}/")))
    .collect::<Vec<_>>();
    match judge(&nulled, &changed, &mut || Inputs::read(repo_root, mock_dir)) {
        Ok(refusals) if refusals.is_empty() => ExitCode::SUCCESS,
        Ok(refusals) => {
            eprintln!("BLOCKED: this commit nulls the green record of a member it does not touch.");
            for r in &refusals {
                eprintln!("  {}: member `{}`, {}", r.path, r.member, r.why);
            }
            eprintln!(
                "  A run cut short leaves a record like this. Unstage the file with `git restore \
                 --staged <path>` to keep the green record, or rerun `cargo mock test` so it is \
                 made again, then commit."
            );
            ExitCode::from(1)
        },
        Err(e) => {
            eprintln!(
                "BLOCKED: this commit nulls a green record and the member's inputs could not be \
                 read to say whether that is warranted: {e}"
            );
            ExitCode::from(1)
        },
    }
}

#[cfg(test)]
#[path = "history_guard_tests.rs"]
mod tests;
