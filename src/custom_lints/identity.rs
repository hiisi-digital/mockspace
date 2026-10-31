//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a built lint library is, as a digest of what decides its behaviour.
//!
//! **The identity is made from the inputs, never from the bytes the build produced.** The library
//! embeds the path of every tree it was compiled from (panic locations, `file!()`), so a digest of
//! its bytes names the tree and not the code, and a second checkout of one commit shares nothing
//! with the first. The inputs are the lint sources as content, the generated manifest with the
//! tree's paths taken out, every package it resolves to (a registry or git package by version and
//! source, a path package by the content of its files and never by where they sit), the enabled
//! features of each, the toolchain, the release profile, and the cargo config files the build
//! reads.
//!
//! The identity is a lookup key and does not claim to name every file the compiler reads: a lint
//! may include a file, or a module in a subdirectory, that no list of names here could predict.
//! `depinfo` holds that half, from cargo's own record of the build.
//!
//! holds for: any tree path, any checkout of the same content, any machine with the same
//! toolchain and the same resolution. Does not hold across a resolution that moved a registry
//! package, which is a different identity and so a rebuild, never a wrong answer.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::depinfo::Roots;

/// One package the generated crate builds against, as far as it decides the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Member {
    pub name:     String,
    pub version:  String,
    /// The registry or git source, with its resolved revision. `None` for a path package.
    pub source:   Option<String>,
    pub features: Vec<String>,
    /// The digest of a path package's files. `None` for a package that is its version.
    pub content:  Option<String>,
}

/// Everything the identity is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Inputs {
    pub toolchain: String,
    /// The digest of the cargo config files the build reads (see [`config_digest`]).
    pub config:    String,
    /// The generated `Cargo.toml`, with the tree's paths taken out.
    pub manifest:  String,
    /// Each lint file as `(name, content digest)`.
    pub lints:     Vec<(String, String)>,
    /// The generated collector, written against a fixed lint directory.
    pub collector: String,
    pub members:   Vec<Member>,
}

/// The lookup key of a library, and the directories its files are named by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pub key:   String,
    pub roots: Roots,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Length-prefixed, so no two different field lists hash alike.
fn field(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_le_bytes());
    h.update(bytes);
}

pub(crate) fn digest_inputs(inputs: &Inputs) -> String {
    let mut h = Sha256::new();
    field(&mut h, b"pack-identity-1");
    field(&mut h, b"profile=release");
    field(&mut h, inputs.toolchain.as_bytes());
    field(&mut h, b"config");
    field(&mut h, inputs.config.as_bytes());
    field(&mut h, b"manifest");
    field(&mut h, inputs.manifest.as_bytes());
    field(&mut h, inputs.collector.as_bytes());
    for (name, digest) in &inputs.lints {
        field(&mut h, b"lint");
        field(&mut h, name.as_bytes());
        field(&mut h, digest.as_bytes());
    }
    let mut members: Vec<String> = inputs
        .members
        .iter()
        .map(|m| {
            format!(
                "{}\0{}\0{}\0{}\0{}",
                m.name,
                m.version,
                m.source.as_deref().unwrap_or("path"),
                m.features.join(","),
                m.content.as_deref().unwrap_or("")
            )
        })
        .collect();
    members.sort();
    for m in &members {
        field(&mut h, b"member");
        field(&mut h, m.as_bytes());
    }
    hex(&h.finalize())
}

/// The files git tracks under `root`, as paths relative to it, or `None` where `root` is in no
/// repository (or git cannot be run) and so git cannot say which files belong.
///
/// The index is what is asked, so a file staged but not yet committed is a file of the package, and
/// one that is untracked or ignored is not: a scratch file beside a package is not part of it, and
/// letting one move the identity builds the library again for nothing. A file that is read by the
/// build and tracked by nobody still reaches `depinfo`, which records what the compiler opened.
fn tracked_files(root: &Path) -> Option<std::collections::HashSet<String>> {
    // Without `--full-name` the paths are relative to the directory it runs in, which is `root`.
    let out = output_of_in(root, "git", &["ls-files", "-z", "--cached", "--", "."])?;
    let listed = out
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| std::str::from_utf8(p).ok())
        .map(str::to_string)
        .collect();
    Some(listed)
}

/// The digest of a directory's files: each relative path and its bytes, in name order. Build
/// output (`target`) and git state are not part of a package, and where the directory is in a git
/// repository only the files git tracks are (see [`tracked_files`]).
pub(crate) fn digest_tree(root: &Path) -> std::io::Result<String> {
    let mut h = Sha256::new();
    // A package whose own manifest git does not track is not tracked at all (a pack drafted before
    // its first `git add`), and its manifest is the one file no dep-info lists: it is read whole.
    let tracked = tracked_files(root).filter(|t| t.contains("Cargo.toml"));
    walk(root, root, &mut h, tracked.as_ref())?;
    // A tracked file that is gone from the disk is a change to the package and is hashed as one.
    if let Some(tracked) = &tracked {
        let mut gone: Vec<&String> =
            tracked.iter().filter(|rel| std::fs::symlink_metadata(root.join(rel.as_str())).is_err()).collect();
        gone.sort();
        for rel in gone {
            field(&mut h, b"gone");
            field(&mut h, rel.as_bytes());
        }
    }
    Ok(hex(&h.finalize()))
}

fn walk(
    root: &Path,
    dir: &Path,
    h: &mut Sha256,
    tracked: Option<&std::collections::HashSet<String>>,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let meta = std::fs::symlink_metadata(&path)?;
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        if meta.is_dir() {
            // Build output at the package root only: a module may be called `target` below it.
            if (dir == root && name == "target") || name == ".git" {
                continue;
            }
            walk(root, &path, h, tracked)?;
        } else if tracked.is_some_and(|t| !t.contains(&rel)) {
            continue;
        } else if meta.file_type().is_symlink() {
            // The link as written, and the content behind it where it is a file. A link to a
            // directory is not followed, which could loop; the files read through it are in the
            // record of the build.
            field(h, rel.as_bytes());
            field(h, std::fs::read_link(&path)?.to_string_lossy().as_bytes());
            if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
                field(h, &std::fs::read(&path)?);
            }
        } else if meta.is_file() {
            field(h, rel.as_bytes());
            field(h, &std::fs::read(&path)?);
        }
    }
    Ok(())
}

pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The packages of a `cargo metadata` document the generated crate depends on, without the
/// generated crate itself, whose manifest is the one place the tree's paths are written.
pub(crate) fn members_from_metadata(
    json: &serde_json::Value,
    generated_manifest: &Path,
) -> Result<Vec<Member>, String> {
    let bad = |what: &str| format!("cargo metadata has no {what}");
    let packages = json.get("packages").and_then(|p| p.as_array()).ok_or_else(|| bad("packages"))?;
    let nodes = json
        .pointer("/resolve/nodes")
        .and_then(|p| p.as_array())
        .ok_or_else(|| bad("resolve"))?;
    let mut out = Vec::new();
    for pkg in packages {
        let text = |key: &str| pkg.get(key).and_then(|v| v.as_str());
        let manifest = Path::new(text("manifest_path").ok_or_else(|| bad("manifest_path"))?);
        if manifest == generated_manifest {
            continue;
        }
        let id = text("id").ok_or_else(|| bad("id"))?;
        let mut features: Vec<String> = nodes
            .iter()
            .find(|n| n.get("id").and_then(|v| v.as_str()) == Some(id))
            .and_then(|n| n.get("features"))
            .and_then(|f| f.as_array())
            .map(|f| f.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        features.sort();
        let source = text("source").map(str::to_string);
        let content = match (&source, manifest.parent()) {
            (None, Some(root)) => {
                Some(digest_tree(root).map_err(|e| format!("could not read {}: {e}", root.display()))?)
            },
            _ => None,
        };
        out.push(Member {
            name: text("name").ok_or_else(|| bad("name"))?.to_string(),
            version: text("version").ok_or_else(|| bad("version"))?.to_string(),
            source,
            features,
            content,
        });
    }
    Ok(out)
}

/// What a run of `program args` prints, or `None` when it cannot be run or fails. Run where the
/// build runs, so a `rust-toolchain.toml` there answers the same.
fn output_of(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(program).args(args).env("LC_ALL", "C").output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// [`output_of`] run in `dir`.
fn output_of_in(dir: &Path, program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(program).args(args).current_dir(dir).env("LC_ALL", "C").output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// The cargo config files the build reads: `.cargo/config.toml` and `.cargo/config` in the
/// working directory and every directory above it, then the same two in cargo's home. They carry
/// `[build] rustflags`, `[profile.release]` overrides, a rustc wrapper and the rest of what the
/// build takes from files, none of which a command line pins in full. Only the content counts,
/// in order, so two trees of one commit under one config agree.
pub(crate) fn config_digest(working_dir: &Path, cargo_home: &Path) -> String {
    let mut h = Sha256::new();
    let dirs = working_dir
        .ancestors()
        .map(|d| ("above", d.join(".cargo")))
        .chain(std::iter::once(("home", cargo_home.to_path_buf())));
    for (place, dir) in dirs {
        for name in ["config.toml", "config"] {
            if let Ok(bytes) = std::fs::read(dir.join(name)) {
                field(&mut h, place.as_bytes());
                field(&mut h, name.as_bytes());
                field(&mut h, &bytes);
            }
        }
    }
    hex(&h.finalize())
}

/// The directories the files of a build are named by: each path package, by name and version.
/// The generated crate is left out, as in [`members_from_metadata`].
pub(crate) fn path_roots(json: &serde_json::Value, generated_manifest: &Path) -> Roots {
    let mut roots = Roots::default();
    let packages = json.get("packages").and_then(|p| p.as_array()).into_iter().flatten();
    for pkg in packages {
        let text = |key: &str| pkg.get(key).and_then(|v| v.as_str());
        let (Some(manifest), Some(name), Some(version)) =
            (text("manifest_path"), text("name"), text("version"))
        else {
            continue;
        };
        let manifest = Path::new(manifest);
        if text("source").is_none() && manifest != generated_manifest {
            if let Some(root) = manifest.parent() {
                roots.add(format!("pkg:{name}@{version}"), root);
            }
        }
    }
    roots
}

/// The generated manifest as text with the tree's paths taken out, so two trees of one content
/// write one manifest. What sits at those paths is hashed as the content of a path package.
fn manifest_text(generated_manifest: &Path, repo_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(generated_manifest).ok()?;
    let root = repo_root.to_string_lossy().replace('\\', "/");
    Some(text.replace('\\', "/").replace(&root, "<repo>"))
}

/// Where the inputs of one library are read from.
pub(crate) struct Tree<'a> {
    pub generated_manifest: &'a Path,
    /// The directory the repo's lint files sit in.
    pub lints_dir:          &'a Path,
    /// The repo's lint files as `(name, path)`.
    pub lints:              &'a [(String, PathBuf)],
    /// The generated collector source, with the lint directory taken out.
    pub collector:          String,
    /// The tree whose paths the generated manifest is written with.
    pub repo_root:          &'a Path,
    /// Where the build runs, which decides the config files and `rust-toolchain.toml` it meets.
    pub working_dir:        &'a Path,
    pub cargo_home:         &'a Path,
}

/// Everything the library built from `tree` is made of, and the directories its files are named
/// by, or `None` where any of it cannot be read.
pub(crate) fn collect_inputs(tree: &Tree) -> Option<(Inputs, Roots)> {
    let toolchain = String::from_utf8(output_of("rustc", &["-vV"])?).ok()?;
    let meta = output_of(
        "cargo",
        &["metadata", "--format-version", "1", "--manifest-path", tree.generated_manifest.to_str()?],
    )?;
    let json: serde_json::Value = serde_json::from_slice(&meta).ok()?;
    let members = members_from_metadata(&json, tree.generated_manifest).ok()?;
    let mut roots = path_roots(&json, tree.generated_manifest);
    roots.add("repo", tree.repo_root);
    roots.add("lints", tree.lints_dir);
    roots.allow_absolute(tree.cargo_home);
    let mut lints = Vec::new();
    for (name, path) in tree.lints {
        lints.push((name.clone(), digest_bytes(&std::fs::read(path).ok()?)));
    }
    let inputs = Inputs {
        toolchain,
        config: config_digest(tree.working_dir, tree.cargo_home),
        manifest: manifest_text(tree.generated_manifest, tree.repo_root)?,
        lints,
        collector: tree.collector.clone(),
        members,
    };
    Some((inputs, roots))
}

/// The identity of the library the crate in `tree` would build, or `None` where anything it is
/// made of cannot be read, in which case the caller falls back to the bytes of what was built,
/// which is never wrong and only fails to be shared.
pub(crate) fn of_generated_crate(tree: &Tree) -> Option<Identity> {
    let (inputs, roots) = collect_inputs(tree)?;
    Some(Identity { key: digest_inputs(&inputs), roots })
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "identity_e2e_tests.rs"]
mod e2e;
