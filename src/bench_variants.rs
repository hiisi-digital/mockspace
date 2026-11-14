//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which `variants/` crate a `bench.toml` entry names, and where its build goes.
//!
//! An entry is a short name (`fnv1a`, the crate under `variants/fnv1a`), a path
//! under `variants/`, or a path to the built artifact itself
//! (`../target/release/bench_satfold_seq`). The last kind names the crate only
//! through its library name, and names the directory the harness loads it from,
//! so the crate is found by its `[lib] name` and built with that directory as
//! its target, else a fresh machine builds it somewhere the harness never looks.

use std::fs;
use std::path::{Path, PathBuf};

/// One variant crate to build, and the target directory its entry names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VariantBuild {
    /// The directory under `variants/`.
    pub dir:        String,
    /// Where an artifact-path entry expects the build; `None` builds with
    /// cargo's own choice, which a short name resolves against.
    pub target_dir: Option<PathBuf>,
}

/// What a set of entries builds, and how many of them named no crate.
#[derive(Debug, Default)]
pub(crate) struct VariantPlan {
    pub builds:    Vec<VariantBuild>,
    pub unmatched: usize,
}

impl VariantPlan {
    fn push(&mut self, b: VariantBuild) {
        if !self.builds.contains(&b) {
            self.builds.push(b);
        }
    }

    /// The builds of `dir`, or one with cargo's own target when no entry
    /// named it, so a whole-tree build still builds every crate once.
    pub(crate) fn builds_of(&self, dir: &str) -> Vec<VariantBuild> {
        let named: Vec<VariantBuild> = self.builds.iter().filter(|b| b.dir == dir).cloned().collect();
        if named.is_empty() {
            vec![VariantBuild { dir: dir.to_string(), target_dir: None }]
        } else {
            named
        }
    }
}

/// The library name of each crate under `variants/`, with its directory.
pub(crate) fn lib_names(bench_dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(bench_dir.join("variants")) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.file_name().to_string_lossy().into_owned();
        let Ok(text) = fs::read_to_string(entry.path().join("Cargo.toml")) else {
            continue;
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            continue;
        };
        let lib = doc
            .get("lib")
            .and_then(|l| l.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_string)
            .or_else(|| {
                doc.get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|n| n.replace('-', "_"))
            });
        if let Some(lib) = lib {
            out.push((lib, dir));
        }
    }
    out
}

/// The build one entry asks for, or `None` where it names no crate.
pub(crate) fn build_for(entry: &str, bench_dir: &Path, libs: &[(String, String)]) -> Option<VariantBuild> {
    if !entry.contains('/') {
        return Some(VariantBuild { dir: entry.to_string(), target_dir: None });
    }
    if let Some(rest) = entry.strip_prefix("variants/") {
        let dir = rest.split('/').next()?.to_string();
        return Some(VariantBuild { dir, target_dir: None });
    }
    let path = Path::new(entry);
    let profile_dir = path.parent()?;
    if profile_dir.file_name()? != "release" {
        return None;
    }
    let target_dir = bench_dir.join(profile_dir.parent()?);
    let file = path.file_name()?.to_str()?;
    let stem = match path.extension() {
        Some(_) => {
            let s = path.file_stem()?.to_str()?;
            s.strip_prefix(std::env::consts::DLL_PREFIX).unwrap_or(s)
        },
        None => file,
    };
    let (_, dir) = libs.iter().find(|(lib, _)| lib == stem)?;
    Some(VariantBuild { dir: dir.clone(), target_dir: Some(target_dir) })
}

/// The plan for the named benches of a `bench.toml`, or for all of them when
/// `names` is empty.
pub(crate) fn plan_for(bench_dir: &Path, names: &[&str]) -> Result<VariantPlan, String> {
    let text = fs::read_to_string(bench_dir.join("bench.toml")).map_err(|e| format!("reading bench.toml: {e}"))?;
    let doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("parsing bench.toml: {e}"))?;
    let bench = doc
        .get("bench")
        .and_then(|b| b.as_table())
        .ok_or_else(|| "bench.toml has no [bench.*] sections".to_string())?;
    let libs = lib_names(bench_dir);
    let all: Vec<&str> = bench.iter().map(|(k, _)| k).collect();
    let names: &[&str] = if names.is_empty() { &all } else { names };
    let mut plan = VariantPlan::default();
    for name in names {
        let section = bench
            .get(name)
            .ok_or_else(|| format!("bench `{name}` not found in bench.toml. Available: {}", all.join(", ")))?;
        let mut entries: Vec<String> = Vec::new();
        let mut take = |item: Option<&toml_edit::Item>| {
            if let Some(arr) = item.and_then(|v| v.as_array()) {
                entries.extend(arr.iter().filter_map(|v| v.as_str()).map(str::to_string));
            }
        };
        take(section.get("variants"));
        if let Some(sizes) = section.get("sizes") {
            if let Some(arr) = sizes.as_array_of_tables() {
                for t in arr.iter() {
                    take(t.get("variants"));
                }
            }
            if let Some(arr) = sizes.as_array() {
                for v in arr.iter() {
                    if let Some(t) = v.as_inline_table() {
                        if let Some(tv) = t.get("variants").and_then(|x| x.as_array()) {
                            entries.extend(tv.iter().filter_map(|e| e.as_str()).map(str::to_string));
                        }
                    }
                }
            }
        }
        for e in &entries {
            match build_for(e, bench_dir, &libs) {
                Some(b) => plan.push(b),
                None => plan.unmatched += 1,
            }
        }
    }
    Ok(plan)
}

#[cfg(test)]
#[path = "bench_variants_tests.rs"]
mod tests;
