//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The digests of files already taken, found again by path, size, modification time and, on unix,
//! change time and inode.
//!
//! Asking a saved trace whether a file moved would cost a read of the file if every digest had to
//! be taken anew. The table keeps the digest each file had with the size and time it had then, so
//! a file whose size and time did not move is answered without being read. The time is only how a
//! digest is found again: the trace holds the digest, so a file put back to what it was, under a
//! new time, is read once and has the digest it had.
//!
//! A time is only as fine as the file system keeps it, so a file written twice within one tick of
//! its clock keeps one time for two contents. A digest taken within [`GRANULARITY`] of the file's
//! time is therefore not trusted on a later run, and the file is read and digested again. This is
//! the problem git calls racy entries, solved the same way.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::disk::Stat;
use crate::verdict::VerdictCache;

/// How close a digest may be taken to the file's own time and still be trusted later. Coarser
/// than any file system this runs on keeps times.
pub(crate) const GRANULARITY: Duration = Duration::from_secs(2);

const FORMAT: &str = "file-digests-v2";

#[derive(Clone)]
struct Row {
    size:   u64,
    mtime:  u128,
    /// The change time and inode on unix, `-` where there are none.
    ident:  String,
    taken:  u128,
    digest: String,
}

/// The table for one repository root, loaded from the store and written back once.
pub(crate) struct Table {
    key:     String,
    rows:    Mutex<HashMap<String, Row>>,
    changed: AtomicBool,
}

fn nanos(t: SystemTime) -> Option<u128> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_nanos())
}

/// A file's change time and inode as a table column, `-` where the system gives none.
fn ident(stat: &Stat) -> String {
    stat.identity.map_or_else(|| "-".to_string(), |(ctime, ino)| format!("{ctime}:{ino}"))
}

impl Table {
    /// The table kept for `root` in `cache`, empty where none is kept or it is in no shape this
    /// version writes.
    pub(crate) fn load(cache: &dyn VerdictCache, root: &str) -> Self {
        let key = cache.digest(&[FORMAT.as_bytes(), root.as_bytes()]);
        let mut rows = HashMap::new();
        if let Some(text) = cache.replay(&key) {
            let mut lines = text.lines();
            if lines.next() == Some(FORMAT) {
                for line in lines {
                    let f: Vec<&str> = line.split('\t').collect();
                    let [path, size, mtime, ident, taken, digest] = f[..] else { continue };
                    let (Ok(size), Ok(mtime), Ok(taken)) = (size.parse(), mtime.parse(), taken.parse()) else {
                        continue;
                    };
                    let Some(path) = super::question::unescape(path) else { continue };
                    rows.insert(path, Row {
                        size,
                        mtime,
                        ident: ident.to_string(),
                        taken,
                        digest: digest.to_string(),
                    });
                }
            }
        }
        Self {
            key,
            rows: Mutex::new(rows),
            changed: AtomicBool::new(false),
        }
    }

    /// The digest kept for `path`, when the file still has the size, the modification time and,
    /// on unix, the change time and inode it had then, and the digest was taken far enough after
    /// the modification time to be trusted. The change time is what a rewrite that keeps the size
    /// and sets the modification time back (`cp -p`, `tar`, `touch -d`) cannot put back.
    pub(crate) fn trusted(&self, path: &str, stat: &Stat) -> Option<String> {
        let mtime = nanos(stat.modified?)?;
        let ident = ident(stat);
        let rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        let row = rows.get(path)?;
        let racy = row.taken < mtime + GRANULARITY.as_nanos();
        (row.size == stat.size && row.mtime == mtime && row.ident == ident && !racy).then(|| row.digest.clone())
    }

    /// Keep `digest` for `path`, taken at `taken` of a file whose size and time were `stat`.
    pub(crate) fn put(&self, path: &str, stat: &Stat, taken: SystemTime, digest: &str) {
        let (Some(modified), Some(taken)) = (stat.modified.and_then(nanos), nanos(taken)) else {
            return;
        };
        let mut rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        let row = Row {
            size: stat.size,
            mtime: modified,
            ident: ident(stat),
            taken,
            digest: digest.to_string(),
        };
        let same = rows.get(path).is_some_and(|r| {
            r.size == row.size && r.mtime == row.mtime && r.ident == row.ident && r.taken == row.taken && r.digest == row.digest
        });
        if !same {
            rows.insert(path.to_string(), row);
            self.changed.store(true, Ordering::Release);
        }
    }

    /// Write the table back when anything in it changed.
    pub(crate) fn save(&self, cache: &dyn VerdictCache) {
        if !self.changed.load(Ordering::Acquire) {
            return;
        }
        let rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        let mut paths: Vec<&String> = rows.keys().collect();
        paths.sort();
        let mut out = String::from(FORMAT);
        out.push('\n');
        for p in paths {
            let r = &rows[p];
            let mut esc = String::new();
            super::question::escape_into(p, &mut esc);
            out.push_str(&format!("{esc}\t{}\t{}\t{}\t{}\t{}\n", r.size, r.mtime, r.ident, r.taken, r.digest));
        }
        cache.remember(&self.key, &out);
    }
}
