//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The files the compiler opened to make a library, which is what the library was made from.
//!
//! **Cargo's dep-info is the list, and nothing here assembles another.** The `.d` file beside a
//! built library names every source rustc read, an `include_str!` and a `#[path]` module in a
//! subdirectory among them, and every environment variable it read. A kept library carries that
//! list with the digest of each file and the value of each variable, and a tree asking for it
//! re-checks them first: the library is loaded only where every one of them is what it was built
//! from.
//!
//! A file is named by the root it sits under (the lint directory, a path package, the repository)
//! and the path below it, as written and not as resolved, so two trees of one content name the
//! same files even where a link inside the tree points elsewhere. A file under no root is named
//! absolutely only where it sits in a directory declared to be the same for every tree (cargo's
//! home); anywhere else the list cannot be trusted and nothing is kept.
//!
//! The variables are compared where the build was given them (`build::keeps_env`). One it was not
//! given reads the same in every build, or is set by cargo from the generated manifest, which the
//! identity holds.
//!
//! A source is trusted only if its change time, which no tool can set back, is older than the
//! build by the margin. That is the part of the compiler's read that a hash taken afterwards
//! cannot see: an edit between the read and the hash.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::build::{Built, Unit, keeps_env};
use super::identity::digest_bytes;

/// How long before a build a source must have last changed for it to be trusted. Filesystems with
/// a coarse timestamp (a second, two on some) stamp a file after a moment it was written in.
pub(crate) const TRUST_MARGIN: Duration = Duration::from_secs(2);

const SEPARATOR: char = '\t';
const ENV: &str = "env";
const UNSET: &str = "unset";

/// The directories a tree's files are named relative to, by label, and the ones whose absolute
/// paths are the same for every tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Roots {
    named:    Vec<(String, PathBuf)>,
    absolute: Vec<PathBuf>,
}

impl Roots {
    /// A root, under the path as written and the path as the filesystem resolves it, so a file
    /// is found by whichever spelling the compiler listed it under.
    pub(crate) fn add(&mut self, label: impl Into<String>, path: &Path) {
        let label = label.into();
        if let Ok(resolved) = path.canonicalize() {
            if resolved != path {
                self.named.push((label.clone(), resolved));
            }
        }
        self.named.push((label, path.to_path_buf()));
    }

    /// A directory whose files are named by absolute path, because they are the same in every
    /// tree.
    pub(crate) fn allow_absolute(&mut self, path: &Path) {
        self.absolute.push(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    }

    /// Each root as a prefix the compiler is told to write as `/<label>`, shallowest first, so the
    /// deepest root is the last to match and wins. A path a library embeds (`file!()`, a panic
    /// location) then reads the same in every tree.
    pub(crate) fn remaps(&self) -> Vec<(PathBuf, String)> {
        let mut out: Vec<(PathBuf, String)> =
            self.named.iter().map(|(label, path)| (path.clone(), format!("/{label}"))).collect();
        out.sort_by_key(|(path, _)| path.components().count());
        out
    }

    fn resolve(&self, label: &str) -> Option<&Path> {
        self.named.iter().find(|(l, _)| l == label).map(|(_, p)| p.as_path())
    }

    fn deepest(&self, file: &Path) -> Option<(&str, &Path)> {
        self.named
            .iter()
            .filter(|(_, root)| file.starts_with(root))
            .max_by_key(|(_, root)| root.components().count())
            .map(|(label, root)| (label.as_str(), root.as_path()))
    }

    /// The name of `file` in this tree, or `None` where it is under no root and not in a
    /// directory the same for every tree.
    fn name(&self, file: &Path) -> Option<String> {
        let resolved = file.canonicalize().ok();
        let by_root = self
            .deepest(file)
            .or_else(|| resolved.as_deref().and_then(|r| self.deepest(r)))
            .map(|(label, root)| {
                let seen = if file.starts_with(root) { file } else { resolved.as_deref().unwrap_or(file) };
                format!("{label}{SEPARATOR}{}", slashed(seen.strip_prefix(root).unwrap_or(seen)))
            });
        by_root.or_else(|| {
            let seen = resolved.as_deref().unwrap_or(file);
            self.absolute
                .iter()
                .any(|dir| seen.starts_with(dir))
                .then(|| format!("abs{SEPARATOR}{}", slashed(seen)))
        })
    }

    fn locate(&self, name: &str) -> Option<PathBuf> {
        let (label, rel) = name.split_once(SEPARATOR)?;
        if label == "abs" {
            return Some(PathBuf::from(rel));
        }
        Some(self.resolve(label)?.join(rel))
    }
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// One thing the library was built from, and what it was: the digest of a file, or of the value of
/// an environment variable (`unset` where it had none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dep {
    name:   String,
    digest: String,
}

/// The sources a rustc dep-info document names. Its first rule is `<output>: <sources>`, with a
/// space in a path written `\ `, and its comment lines are not sources.
pub(crate) fn sources_of(info: &str) -> Vec<PathBuf> {
    let Some(rule) = info.lines().find(|l| !l.starts_with('#') && l.contains(": ")) else {
        return Vec::new();
    };
    let Some((_, list)) = rule.split_once(": ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = list.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                current.push(' ');
                chars.next();
            },
            ' ' => {
                if !current.is_empty() {
                    out.push(PathBuf::from(std::mem::take(&mut current)));
                }
            },
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(PathBuf::from(current));
    }
    out
}

/// The `# env-dep:` lines of a rustc dep-info document: each variable it read, and its value.
fn env_reads(info: &str) -> Vec<(String, Option<String>)> {
    info.lines()
        .filter_map(|l| l.strip_prefix("# env-dep:"))
        .map(|rest| match rest.split_once('=') {
            Some((key, value)) => (key.to_string(), Some(value.to_string())),
            None => (rest.to_string(), None),
        })
        .collect()
}

fn env_digest(value: Option<&[u8]>) -> String {
    value.map_or_else(|| UNSET.to_string(), digest_bytes)
}

/// When a file last changed, by anything: the later of its modification and its change time. The
/// second cannot be set from user space, so `cp -p`, `tar x` and `rsync -t` do not move it back.
fn changed_at(meta: &std::fs::Metadata) -> Option<SystemTime> {
    let modified = meta.modified().ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let secs = u64::try_from(meta.ctime()).ok()?;
        let changed = SystemTime::UNIX_EPOCH + Duration::new(secs, u32::try_from(meta.ctime_nsec()).ok()?);
        return Some(modified.max(changed));
    }
    #[cfg(not(unix))]
    Some(modified)
}

/// How long before the build a unit's dep-info may be stamped and still be the one it wrote.
const SLACK: Duration = Duration::from_secs(2);

/// Variables that name the directory of a package's manifest. A path package or tool crate sits in
/// the tree and its value would carry that tree into a library shared by others.
const MANIFEST_KEYS: [&str; 2] = ["CARGO_MANIFEST_DIR", "CARGO_MANIFEST_PATH"];

/// What the library at `dylib` was built from, with digests, or `None` where the list cannot be
/// trusted: no dep-info, one left by another build, a file unreadable or under no root, one that
/// changed within `margin` of the build, or a unit that read the directory of a manifest in the
/// tree.
///
/// `gen_dir` is where rustc ran, which a relative path in the list is read against. Its files are
/// not inputs, since the identity holds their text, and nor are the build's own outputs, which
/// sit in the target directory above the library. `started` is the moment before cargo was run;
/// where cargo reused an earlier build instead (its dep-info is older), the sources are held to
/// that build's time, so a file changed since cannot pass for one that was built from. `built.units`
/// are the files cargo reported making, whose dep-info carries the variables each unit read.
pub(crate) fn record(built: &Built, gen_dir: &Path, roots: &Roots, margin: Duration) -> Option<Vec<Dep>> {
    let dylib = built.dylib.as_path();
    let units = built.units.as_slice();
    let info = std::fs::read_to_string(info_of(dylib)?).ok()?;
    let own_info = unit_info_of(dylib)?;
    let own_at = std::fs::metadata(&own_info).and_then(|m| m.modified()).ok()?;
    // A build cargo ran wrote the unit's dep-info after it began; one older than that is a file an
    // earlier build left, and the sources are held to the time of the one that made the output.
    if !built.fresh && own_at + SLACK < built.started {
        return None;
    }
    let reference = if built.fresh { own_at } else { built.started };
    let gen_seen = gen_dir.canonicalize().unwrap_or_else(|_| gen_dir.to_path_buf());
    let outside: Vec<PathBuf> = [Some(gen_dir), dylib.parent().and_then(Path::parent)]
        .into_iter()
        .flatten()
        .map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf()))
        .collect();
    // Code that runs while building and belongs to the tree writes files no dep-info lists.
    if units.iter().any(|u| u.tree_code) {
        return None;
    }
    let mut deps = Vec::new();
    let mut take = |info: &str, reference: SystemTime| -> Option<()> {
        for source in sources_of(info) {
            let file = if source.is_absolute() { source } else { gen_dir.join(source) };
            let seen = file.canonicalize().unwrap_or_else(|_| file.clone());
            if outside.iter().any(|dir| seen.starts_with(dir)) {
                continue;
            }
            // The bytes first and the change time after: an edit between the two shows in the time,
            // and the other order would store the new content beside the library of the old.
            let bytes = std::fs::read(&file).ok()?;
            #[cfg(test)]
            super::store::knobs::run_between_read_and_stat(&file);
            let changed = changed_at(&std::fs::metadata(&file).ok()?)?;
            if changed + margin > reference {
                return None;
            }
            let name = roots.name(&file)?;
            if name.contains('\n') {
                return None;
            }
            deps.push(Dep { name, digest: digest_bytes(&bytes) });
        }
        Some(())
    };
    take(&info, reference)?;
    // Each unit's own sources are held to that unit's own time: one cargo reused is only as old as
    // the dep-info it wrote, however recently the library above it was linked.
    let mut infos = vec![own_info];
    for unit in units {
        let path = info_of_unit(unit)?;
        let text = std::fs::read_to_string(&path).ok()?;
        let at = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        take(&text, if unit.fresh { at } else { built.started })?;
        infos.push(path);
    }
    infos.sort();
    infos.dedup();
    for path in infos {
        let unit_info = std::fs::read_to_string(&path).ok()?;
        for (key, value) in env_reads(&unit_info) {
            if let (true, Some(value)) = (MANIFEST_KEYS.contains(&key.as_str()), value.as_deref()) {
                let at = Path::new(value);
                let at = at.canonicalize().unwrap_or_else(|_| at.to_path_buf());
                if !at.starts_with(&gen_seen) && !roots.absolute.iter().any(|dir| at.starts_with(dir)) {
                    return None;
                }
            }
            deps.push(Dep {
                name:   format!("{ENV}{SEPARATOR}{key}"),
                digest: env_digest(value.as_deref().map(str::as_bytes)),
            });
        }
    }
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    deps.dedup();
    Some(deps)
}

/// The dep-info beside a library: cargo leaves `libname.so` and `libname.d` together. That copy
/// lists the sources of the unit and of what it depends on, and not the variables read.
fn info_of(dylib: &Path) -> Option<PathBuf> {
    Some(dylib.with_extension("d"))
}

/// The dep-info rustc wrote for the unit that made `file`: in `deps/` beside it, or for a file
/// cargo linked up into the profile directory, in the `deps/` below it. It is the one with the
/// `# env-dep:` lines, which cargo leaves out of the copy beside the library.
fn unit_info_of(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let stem = file.file_stem()?.to_str()?;
    let stem = stem.strip_prefix("lib").unwrap_or(stem);
    let deps = if dir.file_name().is_some_and(|n| n == "deps") { dir.to_path_buf() } else { dir.join("deps") };
    let name = format!("{stem}.d");
    // A build script is compiled into `build/<package>/`, with its dep-info beside it.
    let beside = dir.join(&name);
    let in_deps = deps.join(&name);
    Some(if !in_deps.exists() && beside.exists() { beside } else { in_deps })
}

/// The dep-info of a unit. A unit may name several files (a build script has a link and the
/// binary), and the dep-info is named for one of them.
fn info_of_unit(unit: &Unit) -> Option<PathBuf> {
    unit.files
        .iter()
        .filter_map(|f| unit_info_of(f))
        .find(|p| p.exists())
        .or_else(|| unit.files.iter().find_map(|f| build_script_info(f)))
}

/// The sources that changed after a unit cargo called fresh was made. Cargo compares modification
/// times, which a tool can set back, so it took an edited source for the one it built from; touching
/// these makes it build the unit for real.
pub(crate) fn newer_than_their_units(built: &Built, gen_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for unit in built.units.iter().filter(|u| u.fresh) {
        let Some(path) = info_of_unit(unit) else { continue };
        let Some(made) = std::fs::metadata(&path).and_then(|m| m.modified()).ok() else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for source in sources_of(&text) {
            let file = if source.is_absolute() { source } else { gen_dir.join(source) };
            if std::fs::metadata(&file).ok().and_then(|m| changed_at(&m)).is_some_and(|at| at > made) {
                out.push(file);
            }
        }
    }
    out
}

/// The dep-info of a build script, which cargo reports as `build/<package>/build-script-build` while
/// rustc wrote `build_script_build-<hash>.d` beside it.
fn build_script_info(file: &Path) -> Option<PathBuf> {
    if file.file_name()? != "build-script-build" {
        return None;
    }
    std::fs::read_dir(file.parent()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "d"))
}

/// Whether every file is, in this tree, what the library was built from, and every variable the
/// build was given reads as it did.
pub(crate) fn hold(deps: &[Dep], roots: &Roots) -> bool {
    deps.iter().all(|dep| match dep.name.split_once(SEPARATOR) {
        Some((ENV, key)) if !keeps_env(key) => true,
        Some((ENV, key)) => {
            let now = std::env::var_os(key);
            env_digest(now.as_deref().map(|v| v.as_encoded_bytes())) == dep.digest
        },
        _ => roots
            .locate(&dep.name)
            .and_then(|path| std::fs::read(path).ok())
            .is_some_and(|bytes| digest_bytes(&bytes) == dep.digest),
    })
}

pub(crate) fn encode(deps: &[Dep]) -> String {
    let mut out = String::from("mockspace-pack-deps-1\n");
    for dep in deps {
        out.push_str(&format!("{}{SEPARATOR}{}\n", dep.digest, dep.name));
    }
    out
}

pub(crate) fn decode(text: &str) -> Option<Vec<Dep>> {
    let mut lines = text.lines();
    if lines.next()? != "mockspace-pack-deps-1" {
        return None;
    }
    lines
        .map(|line| {
            let (digest, name) = line.split_once(SEPARATOR)?;
            Some(Dep { name: name.to_string(), digest: digest.to_string() })
        })
        .collect()
}

#[cfg(test)]
#[path = "depinfo_tests.rs"]
mod tests;
