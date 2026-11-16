//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Built lint libraries kept by identity and by the files they were built from.
//!
//! A library is kept under the key `identity` made for it, so a second tree of the same content
//! loads the first one's build and neither rebuilds nor relinks. The key names the inputs that are
//! known before a build. The files the compiler opened (`depinfo`) are known only after it, so
//! they are kept beside the library and checked against the asking tree before it is loaded: a
//! stale entry is unreachable and never wrong, and [`reap`] removes the ones nothing has used for
//! a while.
//!
//! An entry is two files with one name, `<key>-<list>.<ext>` and `<key>-<list>.deps`, where
//! `<list>` is the digest of the file list. The name ties a list to its own digest and not to the
//! library's bytes. What keeps a library paired with its own list is the lock on the generated crate
//! (`custom_lints::build_and_load`), which a build and the writing of its entry are both inside, and
//! the order of the writes: the library is renamed in first, so a list never names a missing one.
//! A key may hold several entries, one for each state of the files it was built from.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::build::Built;
use super::depinfo::{self, Roots};
use super::identity::digest_bytes;

const DIR: &str = "built";
const EXTENSIONS: [&str; 3] = ["so", "dylib", "dll"];
const LIST: &str = "deps";

/// A library to load, and the identity of the code in it where that is known.
#[derive(Debug)]
pub(crate) struct Library {
    pub path:     PathBuf,
    /// Names exactly the inputs and the files read, or `None` for a library whose build cannot
    /// be tied to them, which is identified by its bytes instead.
    pub identity: Option<String>,
}

fn kept_library(base: &Path, name: &str) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|e| base.join(DIR).join(format!("{name}.{e}")))
        .find(|p| p.is_file())
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// The kept entry for `key` whose files are, in the asking tree, what it was built from.
fn find(base: &Path, key: &str, roots: &Roots) -> Option<(PathBuf, String)> {
    let prefix = format!("{key}-");
    let suffix = format!(".{LIST}");
    for entry in std::fs::read_dir(base.join(DIR)).ok()?.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some(list) = file.strip_prefix(&prefix).and_then(|f| f.strip_suffix(&suffix)) else {
            continue;
        };
        let name = format!("{key}-{list}");
        let Some(deps) = std::fs::read_to_string(entry.path()).ok().and_then(|t| depinfo::decode(&t))
        else {
            continue;
        };
        if let Some(library) = kept_library(base, &name).filter(|_| depinfo::hold(&deps, roots)) {
            touch(&entry.path());
            return Some((library, name));
        }
    }
    None
}

/// The library for `key`: a kept one whose files hold in this tree, stamped as used, or what
/// `build` makes, kept first. A library that cannot be kept is still returned, so a read-only
/// store costs sharing only.
///
/// `gen_dir` is where the build ran, and `margin` how long before it a source must have last
/// changed to be trusted (`depinfo::TRUST_MARGIN`). `unchanged` is asked after a build and says
/// whether the inputs the key was made from still read the same, which is whether the new library
/// may be kept under it.
pub(crate) fn dylib_for(
    base: &Path,
    key: &str,
    revision: &str,
    roots: &Roots,
    gen_dir: &Path,
    margin: Duration,
    unchanged: impl Fn() -> bool,
    mut build: impl FnMut() -> Result<Built, String>,
) -> Result<Library, String> {
    if let Some((path, name)) = find(base, key, roots) {
        touch(&path);
        return Ok(Library { path, identity: Some(name) });
    }
    let mut built = build()?;
    let mut deps = depinfo::record(&built, gen_dir, roots, margin);
    if deps.is_none() && (built.fresh || built.units.iter().any(|u| u.fresh)) {
        // Cargo judged an old output fresh, by timestamps a tool may have set back, and this tree
        // cannot be tied to it. Touching the generated source, and any source newer than the unit
        // cargo reused, makes cargo build them for real.
        for source in depinfo::newer_than_their_units(&built, gen_dir) {
            touch(&source);
        }
        touch(&gen_dir.join("src").join("lib.rs"));
        built = build()?;
        deps = depinfo::record(&built, gen_dir, roots, margin);
    }
    // The key was made before the build ran. What it names but the file list does not hold (the
    // cargo config, the toolchain, the resolved packages) may have moved since, and a library built
    // under the new state must not be kept under the old key.
    let Some(deps) = deps.filter(|_| unchanged()) else {
        return Ok(Library { path: built.dylib, identity: None });
    };
    let list = depinfo::encode(&deps);
    let name = format!("{key}-{}", &digest_bytes(list.as_bytes())[.. 16]);
    if keep(base, &name, &built.dylib, &list) {
        supersede(base, gen_dir, revision, &name);
    }
    Ok(Library { path: built.dylib, identity: Some(name) })
}

/// Library first and list second, each put beside and renamed in, so a peer never maps a
/// half-written library and never finds a list whose library is not there. Whether it was kept.
///
/// The library is a hard link to the file cargo built, never a copy, so the bytes are on disk once.
/// The linker writes a new file and cargo links it into place, so a later build of the same crate
/// leaves the kept file as it was. Where no link can be made (another filesystem), nothing is kept,
/// which costs sharing only.
fn keep(base: &Path, name: &str, built: &Path, list: &str) -> bool {
    let dir = base.join(DIR);
    let ext = built.extension().and_then(|e| e.to_str()).unwrap_or("so");
    let pid = std::process::id();
    let library = dir.join(format!("{name}.{ext}"));
    let tmp_library = dir.join(format!(".{name}.{pid}.tmp"));
    let listing = dir.join(format!("{name}.{LIST}"));
    let tmp_listing = dir.join(format!(".{name}.{pid}.{LIST}.tmp"));
    let _ = std::fs::remove_file(&tmp_library);
    let stored = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::hard_link(built, &tmp_library))
        .and_then(|()| std::fs::rename(&tmp_library, &library))
        .and_then(|()| std::fs::write(&tmp_listing, list))
        .and_then(|()| std::fs::rename(&tmp_listing, &listing));
    if stored.is_err() {
        let _ = std::fs::remove_file(&tmp_library);
        let _ = std::fs::remove_file(&tmp_listing);
    }
    stored.is_ok()
}

/// Which libraries a generated crate kept, one line each: the revision it was built at, a space,
/// and the entry's name.
const KEPT: &str = ".kept";

/// Record `name` as kept by the crate at `gen_dir` at `revision`, and remove every library the
/// crate kept at another revision: a new engine or pack rev has made them old, and nothing of the
/// repo loads them again. Libraries of the same revision stay, one per state of the lint sources,
/// so worktrees of one repo at different sources each find their own.
fn supersede(base: &Path, gen_dir: &Path, revision: &str, name: &str) {
    let record = gen_dir.join(KEPT);
    let mut lines = Vec::new();
    for line in std::fs::read_to_string(&record).unwrap_or_default().lines() {
        let Some((rev, entry)) = line.split_once(' ') else { continue };
        if !is_entry_name(entry) || entry == name {
            continue;
        }
        if rev == revision {
            if kept_library(base, entry).is_some() {
                lines.push(line.to_string());
            }
        } else {
            remove_entry(base, entry);
        }
    }
    lines.push(format!("{revision} {name}"));
    let _ = crate::custom_lints::write_if_changed(&record, &(lines.join("\n") + "\n"));
}

/// A name as `dylib_for` makes them, letters, digits and dashes, so a line of the record can never
/// reach outside the store.
fn is_entry_name(entry: &str) -> bool {
    !entry.is_empty() && entry.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn remove_entry(base: &Path, entry: &str) {
    for ext in EXTENSIONS.iter().chain(std::iter::once(&LIST)) {
        let _ = std::fs::remove_file(base.join(DIR).join(format!("{entry}.{ext}")));
    }
}

/// Remove the kept files unused for longer than `stale_after`, and return how many.
pub(crate) fn reap(base: &Path, now: SystemTime, stale_after: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(base.join(DIR)) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > stale_after));
        if old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
#[path = "built_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "built_e2e_tests.rs"]
mod e2e;
