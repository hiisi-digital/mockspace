//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The directory listings a citation's walk reads, held once while a scope is open.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What one entry of a directory is, as a citation's walk needs it.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) name:    String,
    pub(super) path:    PathBuf,
    pub(super) is_dir:  bool,
    pub(super) is_file: bool,
}

thread_local! {
    /// The directory listings held while a [`CacheScope`] is open, and how many scopes are.
    static LISTINGS: std::cell::RefCell<(usize, HashMap<PathBuf, Rc<[Entry]>>)> =
        std::cell::RefCell::new((0, HashMap::new()));
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// While this is alive, a directory a citation walks through is listed once and the listing held,
/// so a pass that resolves many citations reads each directory once and not once for each of them.
/// Nothing is held outside a scope, so a file made between two calls is found, and a scope is
/// opened around a pass that does not write.
#[must_use]
pub struct CacheScope(());

/// Opens a [`CacheScope`] on this thread. Scopes nest, and the listings go when the outermost ends.
pub fn cache_scope() -> CacheScope {
    LISTINGS.with(|l| l.borrow_mut().0 += 1);
    CacheScope(())
}

impl Drop for CacheScope {
    fn drop(&mut self) {
        LISTINGS.with(|l| {
            let mut l = l.borrow_mut();
            l.0 -= 1;
            if l.0 == 0 {
                l.1.clear();
            }
        });
    }
}

/// How many directories this thread has listed for citations, held ones not counted again.
#[must_use]
pub fn dir_reads() -> usize {
    READS.with(std::cell::Cell::get)
}

pub(super) fn listing(dir: &Path) -> Rc<[Entry]> {
    if let Some(held) = LISTINGS.with(|l| l.borrow().1.get(dir).cloned()) {
        return held;
    }
    READS.with(|r| r.set(r.get() + 1));
    let read: Vec<Entry> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let path = e.path();
                    Entry {
                        name:    e.file_name().to_string_lossy().into_owned(),
                        is_dir:  path.is_dir(),
                        is_file: path.is_file(),
                        path,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let held: Rc<[Entry]> = read.into();
    LISTINGS.with(|l| {
        let mut l = l.borrow_mut();
        if l.0 > 0 {
            l.1.insert(dir.to_path_buf(), held.clone());
        }
    });
    held
}

