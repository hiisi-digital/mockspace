//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a `mock bench run` builds, and what it leaves behind.
//!
//! The arms and the generated driver build in the target directory cargo
//! resolves for the machine, so every dependency they share is compiled once:
//! the one `CARGO_TARGET_DIR` or `build.target-dir` names, and otherwise the
//! mock directory's own `target/`. A run records what it built from cargo's
//! artifact records, and once the driver has exited and the results are
//! written it removes those and nothing else in the target, unless
//! `--keep-builds` asked to keep them for a second run.

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

/// One package cargo built for the run: where its sources are, and what cargo
/// said it wrote.
struct Built {
    name:    String,
    /// How the package's main source reads in a dependency record: the absolute
    /// path, and the path from the package directory, which is how rustc writes a
    /// source under the directory it runs in.
    sources: [Vec<u8>; 2],
    files:   Vec<PathBuf>,
}

/// What a run built, held until the run is over. Dropping it removes the
/// builds unless it was asked to keep them.
pub(crate) struct Builds {
    keep:  bool,
    built: Vec<Built>,
    /// The `target/mock-arms/<bench>/<arm>` directories the driver is pointed at.
    links: Vec<PathBuf>,
    /// `<benches>/target`, pruned when the links leave it empty.
    links_root: PathBuf,
}

impl Builds {
    pub(crate) fn new(bench_dir: &Path, keep: bool) -> Self {
        Self { keep, built: Vec::new(), links: Vec::new(), links_root: bench_dir.join("target") }
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
        for built in self.built.drain(..) {
            remove_built(&built);
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
}

impl Drop for Builds {
    fn drop(&mut self) {
        if !self.keep {
            self.sweep();
        }
    }
}

/// The packages `manifest` built according to cargo's `compiler-artifact`
/// records, with the files each named.
fn artifacts_of(manifest: &Path, stdout: &[u8]) -> Vec<Built> {
    let manifest = manifest.canonicalize().unwrap_or_else(|_| manifest.to_path_buf());
    let manifest_dir = manifest.parent().map(Path::to_path_buf).unwrap_or_default();
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact"))
        .filter(|v| {
            v.get("manifest_path")
                .and_then(|m| m.as_str())
                .is_some_and(|m| Path::new(m).canonicalize().unwrap_or_else(|_| PathBuf::from(m)) == manifest)
        })
        .filter_map(|v| {
            let name = v.get("target")?.get("name")?.as_str()?.to_string();
            let mut files: Vec<PathBuf> = v
                .get("filenames")
                .and_then(|f| f.as_array())
                .map(|a| a.iter().filter_map(|f| f.as_str()).map(PathBuf::from).collect())
                .unwrap_or_default();
            if let Some(exe) = v.get("executable").and_then(|e| e.as_str()) {
                files.push(PathBuf::from(exe));
            }
            let src = PathBuf::from(v.get("target")?.get("src_path")?.as_str()?);
            let rel = src.strip_prefix(&manifest_dir).unwrap_or(&src).to_path_buf();
            let bytes = |p: &Path| p.to_string_lossy().into_owned().into_bytes();
            Some(Built { name, sources: [bytes(&src), bytes(&rel)], files })
        })
        .collect()
}

/// `s` is `<sixteen hex digits>`, cargo's metadata hash.
fn is_hash(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `name` is `<crate>` or `<crate>-<hash>` for a fingerprint directory.
fn names_the_crate(name: &str, crates: &[String]) -> bool {
    crates.iter().any(|c| {
        name == c || name.strip_prefix(c.as_str()).and_then(|r| r.strip_prefix('-')).is_some_and(is_hash)
    })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether the dependency record in `file` names the package's own source.
fn names_the_source(file: &Path, built: &Built) -> bool {
    std::fs::read(file).is_ok_and(|d| built.sources.iter().any(|s| contains(&d, s)))
}

/// Whether `a` and `b` are one file, which is what a hard link makes of the
/// uplifted artifact and the one cargo keeps in `deps/`.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        a.file_name() == b.file_name() && b.is_file()
    }
}

/// Remove what cargo wrote for one package of the run, and only that.
///
/// In `deps/` that is the file the uplifted artifact is a link of, and the
/// `<crate>.d` beside it. In `.fingerprint/` it is the directory of the crate's
/// name whose dependency record names the package's own source. A dependency
/// that shares the arm's crate name has a record naming another source and is
/// left alone, and where one is matched anyway the loss is a rebuild, because
/// cargo takes a missing output for a stale one.
fn remove_built(built: &Built) {
    let underscored = built.name.replace('-', "_");
    let fingerprint_names = [built.name.clone(), underscored.clone()];
    let mut profile_dirs: Vec<PathBuf> =
        built.files.iter().filter_map(|f| f.parent().map(Path::to_path_buf)).collect();
    profile_dirs.sort();
    profile_dirs.dedup();
    for profile in &profile_dirs {
        let deps = profile.join("deps");
        let mut stems: Vec<String> = vec![underscored.clone()];
        if let Ok(entries) = std::fs::read_dir(&deps) {
            for e in entries.flatten() {
                if built.files.iter().any(|f| same_file(f, &e.path())) {
                    if let Some(stem) = e.file_name().to_string_lossy().split('.').next() {
                        stems.push(stem.to_string());
                    }
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        for stem in stems {
            let record = deps.join(format!("{stem}.d"));
            if names_the_source(&record, built) {
                let _ = std::fs::remove_file(record);
            }
        }
        if let Ok(entries) = std::fs::read_dir(profile.join(".fingerprint")) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !names_the_crate(&name, &fingerprint_names) {
                    continue;
                }
                let ours = std::fs::read_dir(e.path()).is_ok_and(|inner| {
                    inner.flatten().any(|f| {
                        f.file_name().to_string_lossy().starts_with("dep-") && names_the_source(&f.path(), built)
                    })
                });
                if ours {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
    // the uplifted files cargo named, and the `.d` it writes beside each
    for f in &built.files {
        let _ = std::fs::remove_file(f);
        let _ = std::fs::remove_file(f.with_extension("d"));
    }
}

#[cfg(test)]
#[path = "bench_builds_tests.rs"]
mod tests;
