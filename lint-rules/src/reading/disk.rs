//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The reading's side of the file system: listings, files, trees and TOML, each made once.
//!
//! This is the only module of the crate's lint side that touches the disk.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::time::SystemTime;

use super::process::{self, Answer};
use super::{Counters, Reading};
use crate::CrateSourceFile;

/// What a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    Other,
}

impl Kind {
    fn of(ft: std::fs::FileType) -> Self {
        if ft.is_dir() {
            Self::Dir
        } else if ft.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }

    pub(crate) fn letter(self) -> &'static [u8] {
        match self {
            Self::File => b"f",
            Self::Dir => b"d",
            Self::Other => b"o",
        }
    }
}

/// What the file system said of a path: its kind, size and modification time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stat {
    pub(crate) kind:     Kind,
    pub(crate) size:     u64,
    /// `None` for an overlaid path, whose content is never found by its time.
    pub(crate) modified: Option<SystemTime>,
    /// On unix, the time of the last change to the file or its inode, in nanoseconds, and its inode
    /// number: what a rewrite that keeps the size and puts the modification time back still moves.
    /// `None` elsewhere and for an overlaid path.
    pub(crate) identity: Option<(i128, u64)>,
}

impl Stat {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            kind:     Kind::of(meta.file_type()),
            size:     meta.len(),
            modified: meta.modified().ok(),
            identity: identity(meta),
        }
    }
}

#[cfg(unix)]
fn identity(meta: &std::fs::Metadata) -> Option<(i128, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()), meta.ino()))
}

#[cfg(not(unix))]
fn identity(_: &std::fs::Metadata) -> Option<(i128, u64)> {
    None
}

/// One name in a directory.
#[derive(Debug, Clone)]
pub struct Entry {
    name: String,
    /// The name as a key knows it, one for one name where the name is not text.
    key:  String,
    stat: Stat,
}

impl Entry {
    /// The entry's file name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether it is a file, a symbolic link to one included.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.stat.kind == Kind::File
    }

    /// Whether it is a directory, a symbolic link to one included.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.stat.kind == Kind::Dir
    }
}

/// A directory's entries, in name order.
///
/// The sizes and times the listing took are kept for finding a digest again and are not lent
/// out: a lint that wants to know what a file holds reads it, and that is in its trace.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    entries: Vec<Entry>,
}

impl Listing {
    /// The entries, in name order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry named `name`, spelled exactly so.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|e| e.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// The bytes a listing's digest is taken over: each name and whether it is a file or a
    /// directory, and nothing of sizes or times, so a file changing that nobody read does not
    /// move it.
    pub(crate) fn material(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for e in &self.entries {
            out.extend_from_slice(e.key.as_bytes());
            out.push(0);
            out.extend_from_slice(e.stat.kind.letter());
            out.push(0);
        }
        out
    }
}

/// A file read: its bytes and what the file system said of it just before.
pub(crate) struct Read {
    pub(crate) bytes: Vec<u8>,
    pub(crate) stat:  Option<Stat>,
    /// When the bytes had been read, which is when their digest describes the file.
    pub(crate) taken: SystemTime,
}

/// A crate's Rust files under `src/`, relative to the crate, the root first, and the directories
/// walked to find them.
pub(crate) struct CrateFiles {
    pub(crate) files:  Vec<PathBuf>,
    pub(crate) listed: Vec<PathBuf>,
}

thread_local! {
    /// Each thread's own parser: a parser cannot be shared, the trees it makes can.
    static PARSER: RefCell<tree_sitter::Parser> = RefCell::new(crate::make_parser());
}

fn parse(text: &str) -> Option<tree_sitter::Tree> {
    crate::timing::time("parse:rust", || PARSER.with(|p| p.borrow_mut().parse(text, None)))
}

/// The tree of the empty source, for a file that has none. One for the process.
pub(crate) fn empty_tree() -> &'static tree_sitter::Tree {
    static EMPTY: std::sync::OnceLock<tree_sitter::Tree> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| parse("").expect("tree-sitter returns a tree for the empty source"))
}

/// Where one file's tree is parsed once for the run: its cell in the reading's memo of trees and
/// the counter of parses. Both are boxed inside the reading, so a `TreeCell` points at nothing
/// that moves when the reading does, which is what lets a [`CrateSourceFile`] the reading keeps
/// hold one.
#[derive(Clone, Copy)]
pub(crate) struct TreeCell<'r> {
    cell:   &'r std::sync::OnceLock<Option<tree_sitter::Tree>>,
    parses: &'r AtomicUsize,
}

impl<'r> TreeCell<'r> {
    pub(crate) fn of(reading: &'r Reading, abs: &Path) -> Self {
        Self {
            cell:   reading.trees.slot(abs.to_path_buf()),
            parses: &reading.counters.parses,
        }
    }

    /// The tree, parsed from `text` by whichever asker comes first; `None` where there is no text.
    pub(crate) fn get<'t>(self, text: impl FnOnce() -> Option<&'t str>) -> Option<&'r tree_sitter::Tree> {
        self.cell
            .get_or_init(|| {
                let text = text()?;
                Counters::bump(self.parses);
                parse(text)
            })
            .as_ref()
    }
}

impl Reading {
    fn overlaid(&self, abs: &Path) -> Option<&str> {
        self.overlay.get(abs).map(String::as_str)
    }

    /// Whether some overlaid path sits below `abs`, which makes `abs` a directory.
    fn holds_overlay(&self, abs: &Path) -> bool {
        self.overlay.keys().any(|p| p != abs && p.starts_with(abs))
    }

    pub(crate) fn stat(&self, abs: &Path) -> Option<&Stat> {
        self.stats
            .get_or(abs.to_path_buf(), || {
                if let Some(text) = self.overlaid(abs) {
                    return Some(Stat {
                        kind:     Kind::File,
                        size:     text.len() as u64,
                        modified: None,
                        identity: None,
                    });
                }
                Counters::bump(&self.counters.stats);
                match std::fs::metadata(abs) {
                    Ok(meta) => Some(Stat::of(&meta)),
                    Err(_) if self.holds_overlay(abs) => Some(Stat {
                        kind:     Kind::Dir,
                        size:     0,
                        modified: None,
                        identity: None,
                    }),
                    Err(_) => None,
                }
            })
            .as_ref()
    }

    pub(crate) fn kind(&self, abs: &Path) -> Option<Kind> {
        self.stat(abs).map(|s| s.kind)
    }

    pub(crate) fn listing(&self, abs: &Path) -> Option<&Listing> {
        self.listings
            .get_or(abs.to_path_buf(), || {
                Counters::bump(&self.counters.listings);
                let mut entries: Vec<Entry> = Vec::new();
                let found = std::fs::read_dir(abs);
                let listed = found.is_ok();
                for entry in found.into_iter().flatten().flatten() {
                    let Ok(ft) = entry.file_type() else { continue };
                    let meta = if ft.is_symlink() { std::fs::metadata(entry.path()) } else { entry.metadata() };
                    let stat = meta.map_or(
                        Stat {
                            kind:     Kind::Other,
                            size:     0,
                            modified: None,
                            identity: None,
                        },
                        |m| Stat::of(&m),
                    );
                    let file_name = entry.file_name();
                    entries.push(Entry {
                        name: file_name.to_string_lossy().into_owned(),
                        key: super::names::text_of(&file_name).into_owned(),
                        stat,
                    });
                }
                // Overlaid files and the directories leading to them are entries too.
                let mut added = false;
                for (path, text) in &self.overlay {
                    let Ok(rest) = path.strip_prefix(abs) else { continue };
                    let mut parts = rest.components();
                    let Some(first) = parts.next() else { continue };
                    let name = first.as_os_str().to_string_lossy().into_owned();
                    let key = super::names::text_of(first.as_os_str()).into_owned();
                    let stat = if parts.next().is_none() {
                        Stat {
                            kind:     Kind::File,
                            size:     text.len() as u64,
                            modified: None,
                            identity: None,
                        }
                    } else {
                        Stat {
                            kind:     Kind::Dir,
                            size:     0,
                            modified: None,
                            identity: None,
                        }
                    };
                    // A file overlaid here replaces what is on disk; a directory leading to one
                    // keeps a directory already there.
                    let keep_existing = stat.kind == Kind::Dir;
                    entries.retain(|e| e.key != key || keep_existing && e.stat.kind == Kind::Dir);
                    if !entries.iter().any(|e| e.key == key) {
                        entries.push(Entry {
                            name,
                            key,
                            stat,
                        });
                    }
                    added = true;
                }
                if !listed && !added {
                    return None;
                }
                entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
                Some(Listing {
                    entries,
                })
            })
            .as_ref()
    }

    pub(crate) fn read(&self, abs: &Path) -> Option<&Read> {
        self.files
            .get_or(abs.to_path_buf(), || {
                if let Some(text) = self.overlaid(abs) {
                    return Some(Read {
                        bytes: text.as_bytes().to_vec(),
                        stat:  None,
                        taken: SystemTime::now(),
                    });
                }
                // The time is taken before the bytes, so a write landing between the two leaves a
                // digest under a time the file no longer has, which is never trusted again.
                let stat = std::fs::metadata(abs).ok().map(|m| Stat::of(&m));
                Counters::bump(&self.counters.reads);
                let bytes = std::fs::read(abs).ok()?;
                Some(Read {
                    bytes,
                    stat,
                    taken: SystemTime::now(),
                })
            })
            .as_ref()
    }

    /// The file as read so far this run, without reading it.
    pub(crate) fn held(&self, abs: &Path) -> Option<&Read> {
        self.files.peek(&abs.to_path_buf())?.as_ref()
    }

    pub(crate) fn bytes(&self, abs: &Path) -> Option<&[u8]> {
        self.read(abs).map(|r| r.bytes.as_slice())
    }

    pub(crate) fn text(&self, abs: &Path) -> Option<&str> {
        std::str::from_utf8(self.bytes(abs)?).ok()
    }

    pub(crate) fn tree(&self, abs: &Path) -> Option<&tree_sitter::Tree> {
        TreeCell::of(self, abs).get(|| self.text(abs))
    }

    /// A tree of `text`, which is no file and is parsed every time it is asked.
    pub(crate) fn parse_text(&self, text: &str) -> tree_sitter::Tree {
        Counters::bump(&self.counters.parses);
        parse(text).unwrap_or_else(|| empty_tree().clone())
    }

    pub(crate) fn toml(&self, abs: &Path) -> Option<&Result<toml::Table, String>> {
        self.tomls
            .get_or(abs.to_path_buf(), || {
                let text = self.text(abs)?;
                Counters::bump(&self.counters.tomls);
                Some(text.parse::<toml::Table>().map_err(|e| e.to_string()))
            })
            .as_ref()
    }

    pub(crate) fn git_answer(&self, cwd: &str, args: &[String]) -> &Answer {
        self.git.get_or((cwd.to_string(), args.to_vec()), || {
            Counters::bump(&self.counters.spawns);
            crate::timing::time("spawn:git", || process::spawn("git", args, &self.path_of(cwd), None))
        })
    }

    pub(crate) fn run_answer(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        stdin: Option<(&str, &[u8])>,
    ) -> &Answer {
        let key = (program.to_string(), args.to_vec(), cwd.to_string(), stdin.map(|(k, _)| k.to_string()));
        self.runs.get_or(key, || {
            Counters::bump(&self.counters.spawns);
            crate::timing::time(&format!("spawn:{program}"), || {
                process::spawn(program, args, &self.path_of(cwd), stdin.map(|(_, b)| b))
            })
        })
    }

    /// What the command a [`Question::Run`] names answered in this run, `None` for any other
    /// question or for a command not run.
    pub(crate) fn ran(&self, q: &super::Question) -> Option<&Answer> {
        let super::Question::Run {
            program,
            args,
            cwd,
            stdin,
            ..
        } = q
        else {
            return None;
        };
        self.runs.peek(&(program.clone(), args.clone(), cwd.clone(), stdin.clone()))
    }

    /// Every file under `dir` the walk keeps, depth first in name order, and every directory it
    /// listed. `skip` is shown each entry's absolute path; an entry it answers `true` for is left
    /// out, and a directory so answered is not entered.
    pub(crate) fn walk(
        &self,
        dir: &Path,
        skip: &dyn Fn(&Path, &Entry) -> bool,
        files: &mut Vec<PathBuf>,
        listed: &mut Vec<PathBuf>,
    ) {
        let Some(listing) = self.listing(dir) else { return };
        listed.push(dir.to_path_buf());
        for e in listing.entries() {
            let path = dir.join(super::names::path_of(&e.key));
            if skip(&path, e) {
                continue;
            }
            if e.is_dir() {
                self.walk(&path, skip, files, listed);
            } else if e.is_file() {
                files.push(path);
            }
        }
    }

    pub(crate) fn crate_files(&self, crate_dir: &Path) -> &CrateFiles {
        self.crate_files.get_or(crate_dir.to_path_buf(), || {
            let mut abs = Vec::new();
            let mut listed = Vec::new();
            let not_rust = |p: &Path, e: &Entry| e.is_file() && p.extension().is_none_or(|x| x != "rs");
            self.walk(&crate_dir.join("src"), &not_rust, &mut abs, &mut listed);
            let lib = Path::new("src/lib.rs");
            let mut files: Vec<PathBuf> =
                abs.iter().filter_map(|p| p.strip_prefix(crate_dir).ok().map(Path::to_path_buf)).collect();
            files.sort_by(|a, b| (a.as_path() != lib, a).cmp(&(b.as_path() != lib, b)));
            CrateFiles {
                files,
                listed,
            }
        })
    }

    /// Every Rust file under `crate_dir/src`, read, with `src/lib.rs` first and the rest in path
    /// order. A file that cannot be read as text is left out.
    #[must_use]
    pub fn crate_sources(&self, crate_dir: &Path) -> &[CrateSourceFile<'_>] {
        let crate_dir = self.absolute(crate_dir);
        let list = self.crate_sources.get_or(crate_dir.clone(), || {
            let found = self.crate_files(&crate_dir);
            let mut out = Vec::with_capacity(found.files.len());
            for rel in &found.files {
                let abs = crate_dir.join(rel);
                let Some(text) = self.text(&abs) else { continue };
                let file = CrateSourceFile::new(rel, text, TreeCell::of(self, &abs));
                // SAFETY: every reference the file holds points into memory the reading boxed and
                // keeps until it is dropped: the path into the boxed `crate_files` entry, the text
                // into the boxed `files` entry, the tree's cell into the boxed `trees` slot and the
                // counter into the boxed counters. None points at the `Reading` itself, which may
                // move after this call, so moving the reading leaves every one of them good. The
                // lifetime is shortened back to the borrow of `self` before anything sees it.
                out.push(unsafe { std::mem::transmute::<CrateSourceFile<'_>, CrateSourceFile<'static>>(file) });
            }
            out
        });
        list.as_slice()
    }
}
