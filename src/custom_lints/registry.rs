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
//! The checksum is of the `.crate` and says nothing of the unpacked copy, so it does not stand
//! alone: the file must also be no newer than the package's `.cargo-ok`, which cargo writes when
//! it has finished unpacking. A file edited by hand afterwards is newer, is held to the age as any
//! source is, and cargo's reuse of the unit built from the old text cannot be mistaken for a build
//! of the edit.
//!
//! holds for: a source under a cargo home declared the same for every tree, of a package the lock
//! gives a checksum, unpacked by a cargo that writes `.cargo-ok`. Does not hold for a path or git
//! package, which have none, and so keep the age. Two registries serving one name and version are
//! keyed alike here, and then the verdict may be a refusal, which is only the age test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::depinfo::changed_at;
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
    ///
    /// `changed` is when `file` last changed. It must be no later than the package's `.cargo-ok`,
    /// which cargo writes when it has finished unpacking: the checksum is of the `.crate` and says
    /// nothing of the unpacked copy, so a file edited after the unpack is not what the checksum names.
    pub(crate) fn trusts(&mut self, file: &Path, changed: SystemTime, homes: &[PathBuf]) -> bool {
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
            let unpacked = home.join("registry").join("src").join(index).join(package);
            let finished = std::fs::metadata(unpacked.join(".cargo-ok")).ok().and_then(|m| changed_at(&m));
            if finished.is_none_or(|finished| changed > finished) {
                return false;
            }
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

/// Whether `file` sits in a cargo home's unpacked registry sources (`registry/src/<index>/`), under
/// whichever home. Only the shape of the path is read, so it holds for a file as the compiler listed it.
pub(crate) fn is_registry_source(file: &Path) -> bool {
    let parts: Vec<_> = file.components().map(|c| c.as_os_str()).collect();
    parts.windows(2).any(|w| w[0] == "registry" && w[1] == "src")
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
