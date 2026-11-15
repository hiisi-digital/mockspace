//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a `mock bench run` builds, and what it leaves behind.
//!
//! The arms and the generated driver build in the target directory cargo
//! resolves for the machine, so every dependency they share is compiled once:
//! the one `CARGO_TARGET_DIR` or `build.target-dir` names, and otherwise the
//! mock directory's own `target/`. A run records each package it built, by the
//! `package_id` of cargo's `compiler-artifact` record and the files that record
//! names, and once the driver has exited and the results are written it removes
//! those files, the `deps/` file each is a link of, and the other files and the
//! fingerprint directory of the metadata hash in that file's name. `--keep-builds`
//! keeps them for a second run.
//!
//! A cdylib has no hash in its file name, so `release/lib<arm>.so` is the same
//! path for every arm of that name in every tree sharing the target. A package
//! whose uplifted file now is one this run did not write is left alone.
//!
//! Two limits of that guard. A build outside the run that writes the same path
//! after cargo exits and before the file's inode is recorded here would be taken
//! for this run's own, and swept with it: the window is the span between one
//! `cargo build` returning and `record` or `link_arm` reading the file, the
//! run holds the tree lock throughout, and another tree's run is not covered by
//! it. Off unix a file has no identity here, so nothing can be told apart and the
//! sweep removes no file at all: the builds stay for the next run.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mockspace_bench_harness::tree as bench_tree;

/// The flag that keeps a run's arm and driver builds.
pub(crate) const KEEP_BUILDS: &str = "--keep-builds";

/// The `--target-dir` a run passes every arm and the driver, or `None` where
/// the machine already fixed one (`inherited`) and cargo must be left to use it.
pub(crate) fn shared_target(mock_dir: &Path, inherited: Option<&OsStr>) -> Option<PathBuf> {
    inherited.is_none().then(|| crate::build_dir::ensure_target_dir(mock_dir))
}

/// `args` without `--keep-builds`, and whether it was there.
pub(crate) fn take_keep_flag<'a>(args: &[&'a str]) -> (Vec<&'a str>, bool) {
    let keep = args.iter().take_while(|a| **a != "--").any(|a| *a == KEEP_BUILDS);
    (args.iter().copied().filter(|a| *a != KEEP_BUILDS).collect(), keep)
}

/// Device and inode of a file: which file it is, whatever its name.
type FileId = (u64, u64);

fn file_id(path: &Path) -> Option<FileId> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// One package cargo built for the run.
struct Built {
    package_id: String,
    /// The library or binary name, underscored: the name of its `.d` in `deps/`.
    crate_name: String,
    /// What cargo said it wrote, each with the file it was when recorded.
    files:      Vec<(PathBuf, Option<FileId>)>,
}

/// What a run built, held until the run is over. Dropping it removes the
/// builds unless it was asked to keep them.
pub(crate) struct Builds {
    keep:    bool,
    built:   Vec<Built>,
    /// The `target/mock-arms/<bench>/<arm>` directories the driver is pointed at.
    links:   Vec<PathBuf>,
    /// `<benches>/target`, pruned when the links leave it empty.
    links_root: PathBuf,
}

impl Builds {
    pub(crate) fn new(bench_dir: &Path, keep: bool) -> Self {
        Self {
            keep,
            built: Vec::new(),
            links: Vec::new(),
            links_root: bench_dir.join("target"),
        }
    }

    /// Remember what the build of `manifest` wrote, from its cargo output.
    pub(crate) fn record(&mut self, manifest: &Path, stdout: &[u8]) {
        self.built.extend(artifacts_of(manifest, stdout));
    }

    /// Put the arm's cdylib where the harness resolves the arm, under the bench
    /// directory's `target/mock-arms/`, as a hard link to what cargo built (a
    /// copy where a link cannot be made), so no second build of it exists. A
    /// later arm of the same library name replaces cargo's own file and leaves
    /// this link whole.
    pub(crate) fn link_arm(
        &mut self,
        bench_dir: &Path,
        arm: &bench_tree::ArmSource,
        manifest: &Path,
        stdout: &[u8],
    ) -> Result<(), String> {
        let built = artifacts_of(manifest, stdout);
        let dylib = built
            .iter()
            .flat_map(|b| b.files.iter())
            .map(|(f, _)| f)
            .find(|f| f.extension().is_some_and(|e| e == std::env::consts::DLL_EXTENSION))
            .cloned()
            .ok_or_else(|| format!("arm {}/arms/{} built no cdylib", arm.bench, arm.arm))?;
        let arm_dir = bench_dir.join(bench_tree::arm_target_dir(&arm.bench, &arm.arm));
        let dest_dir = arm_dir.join("release");
        std::fs::create_dir_all(&dest_dir).map_err(|e| format!("creating {}: {e}", dest_dir.display()))?;
        let dest = dest_dir.join(bench_tree::arm_dylib_name(&arm.arm));
        let _ = std::fs::remove_file(&dest);
        if std::fs::hard_link(&dylib, &dest).is_err() {
            std::fs::copy(&dylib, &dest)
                .map_err(|e| format!("placing {} at {}: {e}", dylib.display(), dest.display()))?;
        }
        self.built.extend(built);
        self.links.push(arm_dir);
        Ok(())
    }

    fn sweep(&mut self) {
        let built = std::mem::take(&mut self.built);
        // every file any package of this run wrote: an arm built after another
        // of the same library name owns the one path they share, and that is
        // still this run's
        let ours: HashSet<FileId> = built.iter().flat_map(|b| b.files.iter()).filter_map(|(_, id)| *id).collect();
        for b in &built {
            let foreign = b.files.iter().any(|(path, recorded)| {
                recorded.is_some() && file_id(path).is_some_and(|now| !ours.contains(&now))
            });
            if foreign {
                eprintln!(
                    "  leaving {}: a build outside this run replaced what it wrote",
                    b.package_id
                );
                continue;
            }
            Self::remove(b);
        }
        for dir in self.links.drain(..) {
            let _ = std::fs::remove_dir_all(&dir);
            // `<arm>`, then `<bench>`, then `mock-arms`, then `target`: each
            // only while it is empty, so anything else in them stays.
            let mut up = dir.parent().map(Path::to_path_buf);
            while let Some(d) = up {
                if !d.starts_with(&self.links_root) || std::fs::remove_dir(&d).is_err() {
                    break;
                }
                up = d.parent().map(Path::to_path_buf);
            }
        }
    }

    /// Remove what the package wrote and what cargo kept beside it, named by
    /// file identity and by the hash in the file names, never by a crate name or
    /// a source path: the uplifted files, the file each is a link of in `deps/`,
    /// and, where that name carries cargo's metadata hash, the other files of the
    /// stem and the fingerprint directory of the hash.
    fn remove(b: &Built) {
        // off unix there is no file identity: leave every file, as for a foreign one
        if !cfg!(unix) {
            return;
        }
        let ids: Vec<FileId> = b.files.iter().filter_map(|(_, id)| *id).collect();
        for (path, _) in &b.files {
            let Some(profile) = path.parent() else { continue };
            let deps = profile.join("deps");
            for e in std::fs::read_dir(&deps).into_iter().flatten().flatten() {
                if file_id(&e.path()).is_some_and(|id| ids.contains(&id)) {
                    remove_stem(profile, &e.file_name().to_string_lossy(), &b.crate_name);
                }
            }
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("d"));
        }
    }
}

/// Remove the `deps/` file `file` and what shares its stem. A name `<stem>-<hash>`
/// is one package's alone, so its other files and the fingerprint directory
/// ending in the hash go too. A name with no hash (a cdylib) is shared by every
/// package of that library name, so only the file itself and the crate's own
/// `.d` go, and its fingerprint directory, whose hash is not in any file name,
/// stays: it is a few small files, and cargo takes a missing output for stale.
fn remove_stem(profile: &Path, file: &str, crate_name: &str) {
    let deps = profile.join("deps");
    let stem = file.split('.').next().unwrap_or(file);
    match stem.rsplit_once('-') {
        Some((_, hash)) if is_hash(hash) => {
            for e in std::fs::read_dir(&deps).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n == stem || n.strip_prefix(stem).is_some_and(|r| r.starts_with('.')) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            for e in std::fs::read_dir(profile.join(".fingerprint")).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.rsplit_once('-').is_some_and(|(_, h)| h == hash) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        },
        _ => {
            let _ = std::fs::remove_file(deps.join(file));
            let _ = std::fs::remove_file(deps.join(format!("{crate_name}.d")));
        },
    }
}

/// `s` is cargo's metadata hash: sixteen hex digits.
fn is_hash(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Drop for Builds {
    fn drop(&mut self) {
        if !self.keep {
            self.sweep();
        }
    }
}

/// The packages `manifest` built according to cargo's `compiler-artifact`
/// records, one per package id, with the files each named.
fn artifacts_of(manifest: &Path, stdout: &[u8]) -> Vec<Built> {
    let manifest = manifest.canonicalize().unwrap_or_else(|_| manifest.to_path_buf());
    let mut out: Vec<Built> = Vec::new();
    for v in String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact"))
        .filter(|v| {
            v.get("manifest_path")
                .and_then(|m| m.as_str())
                .is_some_and(|m| Path::new(m).canonicalize().unwrap_or_else(|_| PathBuf::from(m)) == manifest)
        })
    {
        let Some(package_id) = v.get("package_id").and_then(|p| p.as_str()) else { continue };
        let Some(crate_name) = v.get("target").and_then(|t| t.get("name")).and_then(|n| n.as_str()) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = v
            .get("filenames")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(|f| f.as_str()).map(PathBuf::from).collect())
            .unwrap_or_default();
        if let Some(exe) = v.get("executable").and_then(|e| e.as_str()) {
            paths.push(PathBuf::from(exe));
        }
        let files = paths.into_iter().map(|p| (p.clone(), file_id(&p))).collect::<Vec<_>>();
        match out.iter_mut().find(|b| b.package_id == package_id) {
            Some(b) => b.files.extend(files),
            None => out.push(Built {
                package_id: package_id.to_string(),
                crate_name: crate_name.replace('-', "_"),
                files,
            }),
        }
    }
    out
}

#[cfg(test)]
#[path = "bench_builds_tests.rs"]
mod tests;
