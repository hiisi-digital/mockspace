//! What each file of the directory looked like when it was last hashed, so that a file nobody
//! touched is not read again by the next command.
//!
//! A file is as it was when its size, modification time, change time and inode are, which is how
//! git's own index tells. That alone fails where two writes land in one tick of the clock (a
//! kernel stamps times by the tick, a few milliseconds), so an entry is trusted only when the file
//! was already older than a margin at the time it was hashed: a write after that has a later time
//! than the file had, however it is made. A program can put the modification time back, and cannot
//! put the change time back. What stays open is a clock that moves backwards across the margin, and
//! a filesystem that keeps neither time.
//!
//! The cache is an optimisation and never a source of truth: a file that cannot be read, a line
//! that does not parse and a failed write all leave the next run hashing the file, and none is an
//! error. It sits beside the instance's record in the repository's own directory.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Instance;

const HEADER: &str = "mockspace-stat-cache 1";

/// How much older than the moment it was hashed a file must have been for its entry to be trusted,
/// in seconds. A file system with a coarser time than this (a FAT volume keeps two seconds, and an entry there is trusted too soon) is not
/// one this holds on.
const MARGIN: i64 = 2;

static STAGED: AtomicU64 = AtomicU64::new(0);

/// Size, times and inode of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Stamp {
    size:  u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    ino:   u64,
}

impl Stamp {
    pub(super) fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            size:  meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            ino:   meta.ino(),
        }
    }
}

/// The seconds since the epoch.
pub(super) fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

struct Entry {
    stamp: Stamp,
    id:    Oid,
    at:    i64,
}

#[derive(Default)]
pub(super) struct Stats {
    entries: BTreeMap<String, Entry>,
    changed: bool,
}

impl Stats {
    /// The blob id recorded for `path`, where the file still has the stamp it had and was old
    /// enough when hashed.
    pub(super) fn trusted(&self, path: &str, stamp: &Stamp) -> Option<&Oid> {
        let e = self.entries.get(path)?;
        let old_enough = stamp.mtime.0 + MARGIN <= e.at && stamp.ctime.0 + MARGIN <= e.at;
        (e.stamp == *stamp && old_enough).then_some(&e.id)
    }

    pub(super) fn put(&mut self, path: &str, stamp: Stamp, id: Oid, at: i64) {
        self.entries.insert(
            path.to_owned(),
            Entry {
                stamp,
                id,
                at,
            },
        );
        self.changed = true;
    }

    pub(super) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        let before = self.entries.len();
        self.entries.retain(|p, _| keep(p));
        self.changed |= self.entries.len() != before;
    }
}

fn escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {},
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse(text: &str) -> Stats {
    let mut stats = Stats::default();
    let mut lines = text.split('\n');
    if lines.next() != Some(HEADER) {
        return stats;
    }
    for line in lines.filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.splitn(9, '\t').collect();
        let [id, size, ms, mn, cs, cn, ino, at, path] = f[..] else { continue };
        let num = |s: &str| s.parse::<i64>().ok();
        let (Ok(id), Some(size), Some(ms), Some(mn), Some(cs), Some(cn), Some(ino), Some(at)) =
            (Oid::parse(id), num(size), num(ms), num(mn), num(cs), num(cn), num(ino), num(at))
        else {
            continue;
        };
        stats.entries.insert(
            unescape(path),
            Entry {
                stamp: Stamp {
                    size:  size as u64,
                    mtime: (ms, mn),
                    ctime: (cs, cn),
                    ino:   ino as u64,
                },
                id,
                at,
            },
        );
    }
    stats
}

impl Instance<'_> {
    fn stats_file(&self) -> Result<PathBuf, StoreError> {
        Ok(self.record_file()?.with_extension("stat"))
    }

    /// The cache as it was left, or an empty one where there is none or it cannot be read.
    pub(super) fn load_stats(&self) -> Stats {
        self.stats_file().ok().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| parse(&t)).unwrap_or_default()
    }

    /// Writes the cache where it changed. A failure is the next run's work, not an error.
    pub(super) fn save_stats(&self, stats: &Stats) {
        if !stats.changed {
            return;
        }
        let Ok(file) = self.stats_file() else { return };
        let mut text = format!("{HEADER}\n");
        for (path, e) in &stats.entries {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                e.id,
                e.stamp.size,
                e.stamp.mtime.0,
                e.stamp.mtime.1,
                e.stamp.ctime.0,
                e.stamp.ctime.1,
                e.stamp.ino,
                e.at,
                escape(path)
            ));
        }
        let staged = file.with_extension(format!("{}.{}.tmp", std::process::id(), STAGED.fetch_add(1, Ordering::Relaxed)));
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&staged, text).and_then(|()| std::fs::rename(&staged, &file)).is_err() {
            let _ = std::fs::remove_file(&staged);
        }
    }

    /// Forgets the cache, for a directory made again.
    pub(super) fn forget_stats(&self) {
        if let Ok(file) = self.stats_file() {
            let _ = std::fs::remove_file(file);
        }
    }
}

use crate::error::StoreError;
use crate::ids::Oid;
