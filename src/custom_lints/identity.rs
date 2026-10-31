//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a built lint library is, as a digest of what decides its behaviour.
//!
//! **The identity is made from the inputs, never from the bytes the build produced.** The library
//! embeds the path of every tree it was compiled from (panic locations, `file!()`), so a digest of
//! its bytes names the tree and not the code, and a second checkout of one commit shares nothing
//! with the first. The inputs are the lint sources as content, every package the generated crate
//! resolves to (a registry or git package by version and source, a path package by the content of
//! its files and never by where they sit), the enabled features of each, the toolchain, the
//! release profile and the compiler flags in the environment.
//!
//! holds for: any tree path, any checkout of the same content, any machine with the same
//! toolchain and the same resolution. Does not hold across a resolution that moved a registry
//! package, which is a different identity and so a rebuild, never a wrong answer.

use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

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
    pub flags:     String,
    /// Each lint file as `(name, content digest)`.
    pub lints:     Vec<(String, String)>,
    /// The generated collector, written against a fixed lint directory.
    pub collector: String,
    pub members:   Vec<Member>,
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
    field(&mut h, inputs.flags.as_bytes());
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

/// The digest of a directory's files: each relative path and its bytes, in name order. Build
/// output (`target`) and git state are not part of a package.
pub(crate) fn digest_tree(root: &Path) -> std::io::Result<String> {
    let mut h = Sha256::new();
    walk(root, root, &mut h)?;
    Ok(hex(&h.finalize()))
}

fn walk(root: &Path, dir: &Path, h: &mut Sha256) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            if name == "target" || name == ".git" {
                continue;
            }
            walk(root, &path, h)?;
        } else if meta.is_file() || meta.file_type().is_symlink() {
            // A symlink to a directory has no content to read, and following one could loop.
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            field(h, rel.as_bytes());
            field(h, &bytes);
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

/// The compiler flags the build inherits, which change the library the way a source does.
fn flags_from_env() -> String {
    ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"]
        .iter()
        .map(|k| format!("{k}={}", std::env::var(k).unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The identity of the library the crate at `generated_manifest` would build, or `None` where
/// anything it is made of cannot be read, in which case the caller falls back to the bytes of
/// what was built, which is never wrong and only fails to be shared.
///
/// `lints` are the repo's lint files as `(name, path)` and `collector` is the generated
/// collector source, written against a fixed directory so it names no tree.
pub(crate) fn of_generated_crate(
    generated_manifest: &Path,
    lints: &[(String, std::path::PathBuf)],
    collector: String,
) -> Option<String> {
    let toolchain = String::from_utf8(output_of("rustc", &["-vV"])?).ok()?;
    let meta = output_of(
        "cargo",
        &["metadata", "--format-version", "1", "--manifest-path", generated_manifest.to_str()?],
    )?;
    let json: serde_json::Value = serde_json::from_slice(&meta).ok()?;
    let members = members_from_metadata(&json, generated_manifest).ok()?;
    let mut digests = Vec::new();
    for (name, path) in lints {
        digests.push((name.clone(), digest_bytes(&std::fs::read(path).ok()?)));
    }
    Some(digest_inputs(&Inputs {
        toolchain,
        flags: flags_from_env(),
        lints: digests,
        collector,
        members,
    }))
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "identity_e2e_tests.rs"]
mod e2e;
