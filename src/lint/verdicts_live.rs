//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which salts of a repository have a store open.
//!
//! Every open store holds a shared lock on its salt's mark, `<salt>.live` beside the packed
//! files, for as long as it lives; the kernel lets the lock go when the process ends, however it
//! ends. A salt whose mark another process can lock exclusively has no store open, and its files
//! can go at once.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;

/// The extension of a salt's mark.
pub(super) const LIVE_EXT: &str = "live";

/// The salt a file of the store is named by, when it is one: `<salt>.live`, `<salt>.pack`, or
/// `<salt>.<generation>.pack`.
pub(super) fn salt_of(name: &str) -> Option<&str> {
    let (salt, rest) = name.split_once('.')?;
    let ours = rest == LIVE_EXT
        || rest == super::PACK_EXT
        || rest.strip_suffix(super::PACK_EXT).is_some_and(|g| {
            g.strip_suffix('.')
                .is_some_and(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()))
        });
    (ours && super::is_hex(salt.as_ref(), 64)).then_some(salt)
}

/// Whether `file` is still the file at `path`, and not one a store removed after it was opened.
fn is_at(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (file.metadata(), fs::metadata(path)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        path.exists()
    }
}

fn mark_of(dir: &Path, salt: &str) -> std::path::PathBuf {
    dir.join(format!("{salt}.{LIVE_EXT}"))
}

/// Holds `salt`'s mark in `dir` shared for as long as the returned file lives, making the
/// directory and the mark when missing.
pub(super) fn hold(dir: &Path, salt: &str) -> Option<File> {
    let mark = mark_of(dir, salt);
    for _ in 0 .. 8 {
        let file = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&mark)
        {
            Ok(file) => file,
            // Not made yet, or another process's sweep removed the directory while it was empty.
            Err(e) if e.kind() == ErrorKind::NotFound => {
                fs::create_dir_all(dir).ok()?;
                continue;
            },
            Err(_) => return None,
        };
        file.lock_shared().ok()?;
        // A store that found the mark unheld between this open and this lock removed it, with
        // the salt's files; the mark is made again.
        if is_at(&file, &mark) {
            return Some(file);
        }
    }
    None
}

/// Removes every file of `salt` in `dir`, its mark last, when no store holds the mark, and says
/// whether it did. The mark is locked exclusively throughout, so a store opening the salt
/// meanwhile waits and then makes a mark of its own.
fn drop_salt(dir: &Path, salt: &str) -> bool {
    let mark = mark_of(dir, salt);
    let Ok(file) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&mark)
    else {
        return false;
    };
    if file.try_lock().is_err() || !is_at(&file, &mark) {
        return false;
    }
    if let Ok(files) = fs::read_dir(dir) {
        for f in files.flatten() {
            let name = f.file_name();
            let packed = name
                .to_str()
                .is_some_and(|n| salt_of(n) == Some(salt) && !n.ends_with(LIVE_EXT));
            if packed {
                let _ = fs::remove_file(f.path());
            }
        }
    }
    let _ = fs::remove_file(&mark);
    true
}

/// The salts that have a file in `dir`, and of them those with a packed file.
fn salts_in(dir: &Path) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut all, mut packed) = (BTreeSet::new(), BTreeSet::new());
    for f in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = f.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(salt) = salt_of(name) {
            all.insert(salt.to_string());
            if !name.ends_with(LIVE_EXT) {
                packed.insert(salt.to_string());
            }
        }
    }
    (all, packed)
}

/// Removes every salt in `dir` but `own` that no store holds, at once.
pub(super) fn drop_unheld(dir: &Path, own: &str) {
    for salt in salts_in(dir).0.iter().filter(|s| *s != own) {
        drop_salt(dir, salt);
    }
}

/// Removes the marks in `dir` of salts with no packed file left that no store holds.
pub(super) fn drop_bare_marks(dir: &Path) {
    let (all, packed) = salts_in(dir);
    for salt in all.difference(&packed) {
        drop_salt(dir, salt);
    }
}
