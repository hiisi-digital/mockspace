//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one reading of a run, the views lints are handed of it, and the traces those views write.
//!
//! A [`Reading`] owns everything read while lints run, rooted at the repository: each directory
//! listed, each file read, each Rust file parsed, each TOML file parsed, each `git` question
//! asked and each other command run, every one of them at most once however many lints and
//! threads ask, and only when one first asks. A lint never reaches the file system or starts a
//! process itself. It is handed a [`View`] of the reading for one unit of work and asks through
//! it, and what comes back is borrowed from the reading.
//!
//! The view writes down each question in its [`Trace`]. Saved with the unit's findings, the trace
//! is what a later run asks again: where every answer has the digest it had, the findings are
//! replayed and the lint is not run.

mod disk;
mod each;
mod memo;
pub(crate) mod pool;
mod process;
mod question;
pub(crate) mod store;
mod table;
mod values;
mod view;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub use disk::{Entry, Listing};
pub(crate) use disk::{TreeCell, empty_tree};
pub use each::Item;
pub use process::Answer;
pub(crate) use question::Question;
pub use question::Trace;
pub use store::Store;
pub(crate) use store::UnitKey;
pub(crate) use values::Values;
pub use view::View;

use self::memo::Memo;

/// The variable that sets how many threads a run uses. `1` runs every unit in the order the
/// serial runner always ran them.
pub const THREADS: &str = "MOCKSPACE_LINT_THREADS";

/// The threads a run uses: [`THREADS`] where it is set to a positive number, otherwise the
/// machine's.
#[must_use]
pub fn thread_count() -> usize {
    std::env::var(THREADS)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// How many times the reading went to the disk or to a process, for a test that holds it to once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Directories listed.
    pub listings: usize,
    /// Paths asked of the file system for what they are.
    pub stats:    usize,
    /// Files read.
    pub reads:    usize,
    /// Rust files parsed.
    pub parses:   usize,
    /// TOML files parsed.
    pub tomls:    usize,
    /// Processes started, `git` included.
    pub spawns:   usize,
}

#[derive(Default)]
struct Counters {
    listings: AtomicUsize,
    stats:    AtomicUsize,
    reads:    AtomicUsize,
    parses:   AtomicUsize,
    tomls:    AtomicUsize,
    spawns:   AtomicUsize,
}

impl Counters {
    fn bump(n: &AtomicUsize) {
        n.fetch_add(1, Ordering::Relaxed);
    }
}

/// Everything read during one run, rooted at the repository.
///
/// Lazy and memoised, and `Sync`: any number of threads ask at once, and each thing is produced at
/// most once, by whichever asker reached it first, the others waiting for it. Nothing is read
/// before something asks for it.
///
/// An overlaid reading ([`Reading::with_overlay`]) answers some paths with given text in place of
/// what is on disk. It judges an edit that has not been written yet, and a run over it saves
/// nothing.
pub struct Reading {
    root:     PathBuf,
    overlay:  BTreeMap<PathBuf, String>,
    threads:  usize,
    /// The threads beside the asking one that are free to take work, shared by the run's units and
    /// every `each` inside them.
    pool:     pool::Budget,
    /// Boxed, so a [`crate::CrateSourceFile`] the reading keeps can count its parse here and not
    /// point at the reading itself, which may move.
    counters: Box<Counters>,

    listings:      Memo<PathBuf, Option<Listing>>,
    stats:         Memo<PathBuf, Option<disk::Stat>>,
    files:         Memo<PathBuf, Option<disk::Read>>,
    trees:         Memo<PathBuf, Option<tree_sitter::Tree>>,
    tomls:         Memo<PathBuf, Option<Result<toml::Table, String>>>,
    git:           Memo<(String, Vec<String>), Answer>,
    runs:          Memo<(String, Vec<String>, String, Option<String>), Answer>,
    crate_files:   Memo<PathBuf, disk::CrateFiles>,
    crate_sources: Memo<PathBuf, Vec<crate::CrateSourceFile<'static>>>,
}

impl Reading {
    /// A reading of the repository at `root`, using [`thread_count`] threads.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let threads = thread_count();
        Self {
            root: normalise(&root.into()),
            overlay: BTreeMap::new(),
            threads,
            pool: pool::Budget::new(threads),
            counters:      Box::default(),
            listings:      Memo::new(),
            stats:         Memo::new(),
            files:         Memo::new(),
            trees:         Memo::new(),
            tomls:         Memo::new(),
            git:           Memo::new(),
            runs:          Memo::new(),
            crate_files:   Memo::new(),
            crate_sources: Memo::new(),
        }
    }

    /// This reading with `path` answering `text` in place of whatever is on disk there, a file
    /// that does not exist yet included. Relative paths are taken against the root.
    #[must_use]
    pub fn with_overlay(mut self, path: impl AsRef<Path>, text: impl Into<String>) -> Self {
        let abs = self.absolute(path.as_ref());
        self.overlay.insert(abs, text.into());
        self
    }

    /// This reading with `threads` threads, at least one.
    #[must_use]
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self.pool = pool::Budget::new(self.threads);
        self
    }

    /// The repository root every relative path is taken against.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The run's one budget of threads, which its units and every `each` inside them share.
    pub(crate) fn pool(&self) -> &pool::Budget {
        &self.pool
    }

    /// How many threads the run and every `each` in it use, all together.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Whether any path is overlaid, in which case nothing judged over this reading is saved.
    #[must_use]
    pub fn is_overlaid(&self) -> bool {
        !self.overlay.is_empty()
    }

    /// How often this reading has gone to the disk or started a process so far.
    #[must_use]
    pub fn counts(&self) -> Counts {
        let c = &self.counters;
        Counts {
            listings: c.listings.load(Ordering::Relaxed),
            stats:    c.stats.load(Ordering::Relaxed),
            reads:    c.reads.load(Ordering::Relaxed),
            parses:   c.parses.load(Ordering::Relaxed),
            tomls:    c.tomls.load(Ordering::Relaxed),
            spawns:   c.spawns.load(Ordering::Relaxed),
        }
    }

    /// A view that writes its questions into a trace nobody saves, for a caller that reads through
    /// the reading outside any lint: an engine command, a tool, a test.
    #[must_use]
    pub fn view(&self) -> View<'_> {
        View::new(self, None)
    }

    /// `path` made absolute against the root, with `.` and `..` folded away by name.
    #[must_use]
    pub fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() { normalise(path) } else { normalise(&self.root.join(path)) }
    }

    /// The name a trace writes `path` under: relative to the root with `/` between components when
    /// it is under the root, the absolute path otherwise.
    pub(crate) fn key(&self, path: &Path) -> String {
        let abs = self.absolute(path);
        match abs.strip_prefix(&self.root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => abs.to_string_lossy().into_owned(),
        }
    }

    /// The path a trace's name for it means in this reading.
    pub(crate) fn path_of(&self, key: &str) -> PathBuf {
        let p = Path::new(key);
        if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) }
    }
}

/// `path` with `.` dropped and `..` taking off the component before it, by name only.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            },
            other => out.push(other.as_os_str()),
        }
    }
    out
}
