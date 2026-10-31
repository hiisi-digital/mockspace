//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Registry sources, trusted by the checksum of the crate they were unpacked from.
//!
//! `depinfo` trusts a source only if it changed longer before the build than a margin, which is
//! the part of the compiler's read a hash taken afterwards cannot see. A crate cargo has just
//! unpacked into `registry/src` (a new container, a first build) is as new as the build, so by that
//! test nothing it holds can be kept, and the first load builds twice and keeps no library.
//!
//! A registry crate is not edited between a read and a hash: it is unpacked once from a `.crate`
//! file whose checksum the lock records, and nothing here or in cargo writes to it afterwards. So
//! a file under `registry/src/<index>/<name>-<version>/` is trusted without an age when the
//! `.crate` beside it, in `registry/cache/<index>/`, hashes to the checksum the generated crate's
//! lock gives for that package. Its own digest is still recorded and checked as any source's is.
//!
//! holds for: a source under a cargo home declared the same for every tree, of a package the lock
//! gives a checksum. Does not hold for a path or git package, which have none, and so keep the age.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::identity::digest_bytes;

/// What the lock of one generated crate says, and the verdicts reached so far, so a crate file is
/// hashed once however many of its sources the build read.
pub(crate) struct Registry {
    /// `<name>-<version>` to the checksum of its `.crate`.
    locked:   HashMap<String, String>,
    verdicts: HashMap<PathBuf, bool>,
}

impl Registry {
    /// The checksums in `gen_dir/Cargo.lock`. A lock that cannot be read gives none, and then
    /// nothing is trusted by checksum.
    pub(crate) fn of(gen_dir: &Path) -> Self {
        let locked = std::fs::read_to_string(gen_dir.join("Cargo.lock"))
            .ok()
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
            .map(|doc| checksums(&doc))
            .unwrap_or_default();
        Registry { locked, verdicts: HashMap::new() }
    }

    /// Whether `file`, as the filesystem resolves it, sits in the unpacked source of a registry
    /// crate under one of `homes` whose `.crate` hashes to the checksum the lock gives.
    pub(crate) fn trusts(&mut self, file: &Path, homes: &[PathBuf]) -> bool {
        for home in homes {
            let Ok(below) = file.strip_prefix(home.join("registry").join("src")) else {
                continue;
            };
            let mut parts = below.components();
            let (Some(index), Some(package)) = (parts.next(), parts.next()) else {
                continue;
            };
            let (index, package) = (index.as_os_str(), package.as_os_str());
            let Some(package_name) = package.to_str() else {
                continue;
            };
            let crate_file = home.join("registry").join("cache").join(index).join(format!("{package_name}.crate"));
            if let Some(verdict) = self.verdicts.get(&crate_file) {
                return *verdict;
            }
            let verdict = self.locked.get(package_name).is_some_and(|checksum| {
                std::fs::read(&crate_file).is_ok_and(|bytes| &digest_bytes(&bytes) == checksum)
            });
            self.verdicts.insert(crate_file, verdict);
            return verdict;
        }
        false
    }
}

fn checksums(doc: &toml_edit::DocumentMut) -> HashMap<String, String> {
    let Some(packages) = doc.get("package").and_then(|p| p.as_array_of_tables()) else {
        return HashMap::new();
    };
    packages
        .iter()
        .filter_map(|p| {
            let text = |key: &str| p.get(key).and_then(|v| v.as_str());
            Some((format!("{}-{}", text("name")?, text("version")?), text("checksum")?.to_string()))
        })
        .collect()
}
