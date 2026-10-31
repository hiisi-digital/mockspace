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
/// changed to be trusted (`depinfo::TRUST_MARGIN`).
pub(crate) fn dylib_for(
    base: &Path,
    key: &str,
    roots: &Roots,
    gen_dir: &Path,
    margin: Duration,
    build: impl FnOnce() -> Result<Built, String>,
) -> Result<Library, String> {
    if let Some((path, name)) = find(base, key, roots) {
        touch(&path);
        return Ok(Library { path, identity: Some(name) });
    }
    let built = build()?;
    let Some(deps) = depinfo::record(&built.dylib, gen_dir, roots, built.started, margin)
    else {
        return Ok(Library { path: built.dylib, identity: None });
    };
    let list = depinfo::encode(&deps);
    let name = format!("{key}-{}", &digest_bytes(list.as_bytes())[.. 16]);
    keep(base, &name, &built.dylib, &list);
    Ok(Library { path: built.dylib, identity: Some(name) })
}

/// Library first and list second, each written beside and renamed in, so a peer never maps a
/// half-written library and never finds a list whose library is not there.
fn keep(base: &Path, name: &str, built: &Path, list: &str) {
    let dir = base.join(DIR);
    let ext = built.extension().and_then(|e| e.to_str()).unwrap_or("so");
    let pid = std::process::id();
    let library = dir.join(format!("{name}.{ext}"));
    let tmp_library = dir.join(format!(".{name}.{pid}.tmp"));
    let listing = dir.join(format!("{name}.{LIST}"));
    let tmp_listing = dir.join(format!(".{name}.{pid}.{LIST}.tmp"));
    let stored = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::copy(built, &tmp_library))
        .and_then(|_| std::fs::rename(&tmp_library, &library))
        .and_then(|()| std::fs::write(&tmp_listing, list))
        .and_then(|()| std::fs::rename(&tmp_listing, &listing));
    if stored.is_err() {
        let _ = std::fs::remove_file(&tmp_library);
        let _ = std::fs::remove_file(&tmp_listing);
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
