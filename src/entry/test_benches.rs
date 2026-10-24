//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether `mock test` runs the benches tree.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::suite::fingerprint::{self, WORKSPACE_INPUTS};

/// Why the benches tree runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Run {
    Asked,
    NeverGreen,
    Moved,
}

/// Why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skip {
    OtherCratesNamed,
    Unchanged,
}

impl Skip {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Skip::OtherCratesNamed => "crates were named with -p; --benches runs it too",
            Skip::Unchanged => "its inputs have not moved since it last passed; --benches runs it",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Run(Run),
    Skip(Skip),
}

/// Directory names whose contents are build output, at any depth, whether or
/// not the repository ignores them. Version control is not here: the file list
/// is git's own, and git never lists `.git`.
const OUTPUT_DIRS: [&str; 1] = ["target"];

/// Directories at the bench root that `mock bench run` writes: output, never
/// input.
const GENERATED: [&str; 2] = ["results", "history"];

/// The lockfile the bench build writes beside the tree it builds. One that git
/// does not track is that output and is taken out of the digest, so a run does
/// not move the digest it recorded. One a consumer committed is an input: it
/// says which framework commit the arms are built at (`bench_rev`). The
/// workspace's own lockfiles stay in either way, through [`WORKSPACE_INPUTS`].
const BUILD_LOCKFILE: &str = "Cargo.lock";

/// What the digest is made of, bumped when that changes so a record kept under
/// the old makeup never equals one taken under the new.
const SCHEME: &str = "1";

/// A manifest or source `mock bench` generates under `target/`, which no file
/// listing sees.
pub(crate) struct Generated {
    /// The directory the manifest is read from, for the dependencies it names.
    dir:   Option<PathBuf>,
    /// The entry name it is digested under.
    label: String,
    text:  String,
}

/// A digest of everything the benches tree is built from, or `None` when the
/// inputs cannot be listed (not a git repository), which leaves the tree
/// never shown green. See [`inputs`] for what counts.
pub(crate) fn digest(repo_root: &Path, mock_dir: &Path, bench_dir: &Path) -> Option<String> {
    entries(repo_root, mock_dir, bench_dir)
        .ok()
        .map(digest_entries)
}

/// The named byte strings the digest is taken over.
pub(crate) fn entries(
    repo_root: &Path,
    mock_dir: &Path,
    bench_dir: &Path,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let (files, generated) = inputs(repo_root, mock_dir, bench_dir)?;
    let mut out: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|f| (label(repo_root, f), std::fs::read(f).unwrap_or_default()))
        .collect();
    out.extend(
        generated
            .into_iter()
            .map(|g| (g.label, g.text.into_bytes())),
    );
    Ok(out)
}

/// The digest of named byte strings under the scheme this build uses.
pub(crate) fn digest_entries(entries: Vec<(String, Vec<u8>)>) -> String {
    digest_entries_under(SCHEME, entries)
}

/// The digest of named byte strings under `scheme`. Sorted by name first, so
/// the order the files were listed in is not an input; each name and each body
/// is closed off with a separator so the boundary between them cannot be
/// shifted.
pub(crate) fn digest_entries_under(scheme: &str, mut entries: Vec<(String, Vec<u8>)>) -> String {
    entries.sort();
    entries.dedup();
    let mut h = Sha256::new();
    h.update(b"scheme\0");
    h.update(scheme.as_bytes());
    h.update([0u8]);
    for (name, bytes) in &entries {
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    format!("{:x}", h.finalize())
}

fn label(repo_root: &Path, f: &Path) -> String {
    f.strip_prefix(repo_root)
        .unwrap_or(f)
        .to_string_lossy()
        .into_owned()
}

/// Everything the benches tree is built from, as the files and the generated
/// texts:
///
/// - the files git tracks or would track under `bench_dir`, the same list the
///   members use, less `results/` and `history/` and any lockfile git does not
///   track, which is the one the build writes;
/// - the files of every local package a bench manifest reaches by `path`,
///   transitively, the generated arm manifests and the support crates among
///   them (a workspace-inherited path included), wherever in the repository
///   they sit;
/// - the files every build in the workspace reads, which the members'
///   fingerprints are salted with ([`WORKSPACE_INPUTS`]);
/// - the text of what `mock bench` generates for a tree whose arms have no
///   manifest: the arm manifests and the driver, which no file holds.
pub(crate) fn inputs(
    repo_root: &Path,
    mock_dir: &Path,
    bench_dir: &Path,
) -> Result<(Vec<PathBuf>, Vec<Generated>), String> {
    let tracked = fingerprint::tracked_files(repo_root)?;
    let committed: BTreeSet<PathBuf> = fingerprint::committed_files(repo_root)?
        .into_iter()
        .collect();
    let bench_rel = bench_dir
        .strip_prefix(repo_root)
        .map_err(|_| format!("{} is not under the repository", bench_dir.display()))?;
    let is_input = |rel: &Path| {
        match rel.strip_prefix(bench_rel) {
            Err(_) => true,
            Ok(under) => {
                let first = under
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str());
                !first.is_some_and(|n| GENERATED.contains(&n))
                    && !under
                        .components()
                        .any(|c| OUTPUT_DIRS.iter().any(|o| c.as_os_str() == *o))
                    && (rel.file_name().is_none_or(|n| n != BUILD_LOCKFILE)
                        || committed.contains(rel))
            },
        }
    };

    let mut out: BTreeSet<PathBuf> = tracked
        .iter()
        .filter(|f| f.starts_with(bench_rel) && is_input(f))
        .map(|f| repo_root.join(f))
        .collect();

    // The manifests to read: those on disk under the bench tree, and the ones
    // `mock bench` generates for arms that have none.
    let mut manifests: Vec<(PathBuf, String)> = out
        .iter()
        .filter(|f| f.file_name().is_some_and(|n| n == "Cargo.toml"))
        .filter_map(|f| Some((f.parent()?.to_path_buf(), std::fs::read_to_string(f).ok()?)))
        .collect();
    let generated = generated(mock_dir, bench_dir);
    manifests.extend(
        generated
            .iter()
            .filter_map(|g| Some((g.dir.clone()?, g.text.clone()))),
    );

    let roots = [mock_dir.join("Cargo.toml"), repo_root.join("Cargo.toml")];
    let inherited: Vec<(PathBuf, String)> = roots
        .iter()
        .filter_map(|m| Some((m.parent()?.to_path_buf(), std::fs::read_to_string(m).ok()?)))
        .collect();

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut queue: Vec<(PathBuf, String)> = manifests;
    while let Some((dir, text)) = queue.pop() {
        for dep in path_dependencies(&text, &dir, &inherited) {
            if !seen.insert(dep.clone()) {
                continue;
            }
            match dep.strip_prefix(repo_root) {
                Ok(rel) => {
                    out.extend(
                        tracked
                            .iter()
                            .filter(|f| f.starts_with(rel) && is_input(f))
                            .map(|f| repo_root.join(f)),
                    )
                },
                Err(_) => out.extend(fingerprint::walk_outside(&dep)),
            }
            if let Ok(t) = std::fs::read_to_string(dep.join("Cargo.toml")) {
                queue.push((dep, t));
            }
        }
    }

    for base in [mock_dir, repo_root] {
        for name in WORKSPACE_INPUTS {
            let p = base.join(name);
            if p.is_file() {
                out.insert(p);
            }
        }
    }
    Ok((out.into_iter().collect(), generated))
}

/// What `mock bench` writes under `target/` for a tree with no manifest at its
/// root: a manifest for each arm that has none, and the driver's manifest and
/// source. Digested as text because what they say depends on this build of
/// mockspace as much as on any file the consumer keeps.
fn generated(mock_dir: &Path, bench_dir: &Path) -> Vec<Generated> {
    let mut out = Vec::new();
    if bench_dir.join("Cargo.toml").is_file() {
        return out;
    }
    let Ok(plan) = crate::bench_gen::plan(bench_dir) else {
        return out;
    };
    let dep = crate::bench_gen::mockspace_dep(&plan.manifest);
    for arm in plan.arms.iter().filter(|a| !a.has_manifest) {
        if let Ok(text) = crate::bench_gen::arm_cargo_toml(arm, &dep, &plan.support) {
            out.push(Generated {
                dir: Some(crate::bench_gen::arm_gen_dir(mock_dir, arm)),
                label: format!("generated/{}/{}/Cargo.toml", arm.bench, arm.arm),
                text,
            });
        }
    }
    if let Ok(text) = crate::bench_gen::driver_cargo_toml(&dep, &plan.support) {
        out.push(Generated {
            dir: Some(crate::bench_gen::driver_gen_dir(mock_dir)),
            label: "generated/driver/Cargo.toml".to_string(),
            text,
        });
    }
    if let Ok(text) = crate::bench_gen::driver_main_source(&plan.manifest, None) {
        out.push(Generated {
            dir: None,
            label: "generated/driver/main.rs".to_string(),
            text,
        });
    }
    out
}

/// The directories a manifest depends on by `path`, in its `dependencies`,
/// `dev-dependencies` and `build-dependencies` and the same under
/// `[target.*]`. A dependency written `name.workspace = true` is looked up in
/// the `[workspace.dependencies]` of the manifests in `inherited`, and its path
/// is relative to the manifest that declares it.
fn path_dependencies(text: &str, dir: &Path, inherited: &[(PathBuf, String)]) -> Vec<PathBuf> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut tables: Vec<&toml_edit::Table> = Vec::new();
    for k in KINDS {
        tables.extend(doc.get(k).and_then(|i| i.as_table()));
    }
    for (_, t) in doc
        .get("target")
        .and_then(|t| t.as_table())
        .into_iter()
        .flat_map(|t| t.iter())
    {
        for k in KINDS {
            tables.extend(t.get(k).and_then(|i| i.as_table()));
        }
    }
    let path_of = |item: &toml_edit::Item| -> Option<String> {
        item.get("path")?.as_str().map(str::to_string)
    };
    let mut out = Vec::new();
    for table in tables {
        for (name, item) in table {
            if let Some(p) = path_of(item) {
                out.push(normalise(&dir.join(p)));
            } else if item.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                for (root_dir, root_text) in inherited {
                    let Ok(root) = root_text.parse::<toml_edit::DocumentMut>() else {
                        continue;
                    };
                    let decl = root
                        .get("workspace")
                        .and_then(|w| w.get("dependencies"))
                        .and_then(|d| d.get(name));
                    if let Some(p) = decl.and_then(path_of) {
                        out.push(normalise(&root_dir.join(p)));
                    }
                }
            }
        }
    }
    out
}

/// `a/b/../c` as `a/c`, without touching the disk.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            },
            Component::CurDir => {},
            other => out.push(other),
        }
    }
    out
}

/// The benches tree is a tree like the others: asked for (see
/// `Invocation::benches_asked`), or run when its inputs moved, and never when
/// the caller named crates with `-p` and did not ask. No record means it has
/// never been shown green, so it runs.
pub(crate) fn decide(
    asked: bool,
    crates_named: bool,
    now: &str,
    recorded: Option<&str>,
) -> Verdict {
    if asked {
        return Verdict::Run(Run::Asked);
    }
    if crates_named {
        return Verdict::Skip(Skip::OtherCratesNamed);
    }
    match recorded {
        None => Verdict::Run(Run::NeverGreen),
        Some(r) if r == now => Verdict::Skip(Skip::Unchanged),
        Some(_) => Verdict::Run(Run::Moved),
    }
}

/// [`decide`], reading the digest and the record only where the answer depends
/// on them: asking, or naming crates, settles it before either is read, and
/// taking the digest walks the tree.
///
/// A tree whose inputs cannot be listed has no digest, and with nothing to
/// equal it has never been shown green whatever was recorded.
pub(crate) fn verdict(
    asked: bool,
    crates_named: bool,
    digest: impl FnOnce() -> Option<String>,
    recorded: impl FnOnce() -> Option<String>,
) -> Verdict {
    if asked || crates_named {
        return decide(asked, crates_named, "", None);
    }
    match digest() {
        Some(now) => decide(asked, crates_named, &now, recorded().as_deref()),
        None => decide(asked, crates_named, "", None),
    }
}

/// The digest the benches tree last passed at, kept under `target/` beside the
/// build it describes: a fresh clone has no build and so no record.
pub(crate) fn recorded(mock_dir: &Path) -> Option<String> {
    std::fs::read_to_string(record_path(mock_dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(crate) fn record_green(mock_dir: &Path, digest: &str) {
    let _ = std::fs::write(record_path(mock_dir), format!("{digest}\n"));
}

fn record_path(mock_dir: &Path) -> PathBuf {
    crate::build_dir::ensure_under_target(mock_dir, &["mockspace-test"]).join("benches-green")
}

/// Whether a green run may be recorded: the tree was listed before it and
/// after, and had not moved in between. An edit made while the arms were
/// building was never tested, so it is not recorded as passed.
pub(crate) fn recordable(before: Option<&str>, after: Option<&str>) -> bool {
    matches!((before, after), (Some(b), Some(a)) if a == b)
}

/// The last line of a run, naming the benches tree when it was left out.
pub(crate) fn summary(ran: usize, skipped: Option<Skip>) -> String {
    match skipped {
        None => format!("mock test: {ran} tree(s) green"),
        Some(why) => {
            format!(
                "mock test: {ran} tree(s) green; benches not run ({})",
                why.describe()
            )
        },
    }
}

/// The closing line of a run in which a tree failed, naming the benches tree
/// when it was left out, as [`summary`] does for one that passed.
pub(crate) fn failure_summary(failed: usize, ran: usize, skipped: Option<Skip>) -> String {
    match skipped {
        None => format!("mock test: {failed} of {ran} tree(s) failed"),
        Some(why) => {
            format!(
                "mock test: {failed} of {ran} tree(s) failed; benches not run ({})",
                why.describe()
            )
        },
    }
}

#[cfg(test)]
#[path = "test_benches_tests.rs"]
mod tests;
