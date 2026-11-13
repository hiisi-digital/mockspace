//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a lint is handed of the reading: one unit of work's own way in, which writes down what it
//! asks.

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::disk::{CrateFiles, Kind};
use super::{Answer, Entry, Listing, Question, Reading, Trace, UnitKey};

/// One unit of work's view of the reading.
///
/// Created by the runner for each unit: one lint over one crate or one file, one workspace lint,
/// one repo lint, or one item of a lint that splits itself with [`View::each`]. Everything a lint
/// needs from the disk or from a process goes through here and comes back borrowed from the
/// reading, which outlives every lint of the run. Each call writes down in the view's [`Trace`]
/// what was asked; the digest of the answer is taken from the reading when the trace is saved.
///
/// Relative paths are taken against the repository root, [`View::root`].
pub struct View<'r> {
    reading:          &'r Reading,
    pub(crate) unit:  Option<UnitKey<'r>>,
    trace:            Mutex<Trace>,
    /// How many `each` calls are judging items right now, during which a question put to this view
    /// rather than to an item's own is one the items' traces do not hold.
    pub(crate) busy:  AtomicUsize,
    /// How many questions were put to this view while an `each` was judging items, an `each` of
    /// its own among them. A call whose count moved while it ran saves none of its items. It only
    /// grows, so no call can clear what another call's items asked.
    pub(crate) stray: AtomicUsize,
    /// How many `each` calls this view has made from each place in the source, which tells the
    /// items of one call from another's.
    pub(crate) calls: Mutex<HashMap<(&'static str, u32, u32), usize>>,
}

impl<'r> View<'r> {
    pub(crate) fn new(reading: &'r Reading, unit: Option<UnitKey<'r>>) -> Self {
        Self {
            reading,
            unit,
            trace: Mutex::new(Trace::default()),
            busy: AtomicUsize::new(0),
            stray: AtomicUsize::new(0),
            calls: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn reading(&self) -> &'r Reading {
        self.reading
    }

    pub(crate) fn record(&self, q: Question) {
        if self.busy.load(Ordering::Acquire) > 0 {
            self.stray.fetch_add(1, Ordering::AcqRel);
        }
        self.trace.lock().unwrap_or_else(PoisonError::into_inner).questions.insert(q);
    }

    pub(crate) fn record_value(&self, key: &str) {
        self.record(Question::Value(key.to_string()));
    }

    pub(crate) fn absorb(&self, other: &Trace) {
        self.trace.lock().unwrap_or_else(PoisonError::into_inner).absorb(other);
    }

    /// What this view has written down so far.
    #[must_use]
    pub fn trace(&self) -> Trace {
        self.trace.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn into_trace(self) -> Trace {
        self.trace.into_inner().unwrap_or_else(PoisonError::into_inner)
    }

    /// Say that this unit depends on something a trace cannot hold, an environment variable or the
    /// clock, so it is run on every gate and never replayed. `reason` is kept for whoever reads
    /// why.
    pub fn untraced(&self, reason: &str) {
        let mut t = self.trace.lock().unwrap_or_else(PoisonError::into_inner);
        if t.untraced.is_none() {
            t.untraced = Some(reason.to_string());
        }
    }

    /// The repository root every relative path is taken against.
    ///
    /// The store is shared by every checkout on the machine and a trace holds paths relative to
    /// the root, so a unit is replayed in another checkout of the same files. A unit handed this
    /// absolute path, or one built on it, could carry it into a finding, so asking for it writes the
    /// root into the trace and the unit is then replayed only under the same root. [`Self::walk`]
    /// and [`Self::glob`], which answer with absolute paths, do the same.
    #[must_use]
    pub fn root(&self) -> &'r Path {
        self.record_value("root");
        self.reading.root()
    }

    fn abs(&self, path: &Path) -> PathBuf {
        self.reading.absolute(path)
    }

    // -----------------------------------------------------------------------------------------
    // The file system
    // -----------------------------------------------------------------------------------------

    /// The entries of one directory in name order, `None` where it is not a readable directory.
    /// A file appearing in it or leaving it reruns this unit.
    pub fn list(&self, dir: impl AsRef<Path>) -> Option<&'r Listing> {
        let abs = self.abs(dir.as_ref());
        self.record(Question::List(self.reading.key(&abs)));
        self.reading.listing(&abs)
    }

    /// Every file under `dir`, depth first in name order, absolute. `skip` is shown each entry's
    /// absolute path and the entry: an entry it answers `true` for is left out, and a directory so
    /// answered is not entered. Every directory entered is a listing in the trace.
    pub fn walk(&self, dir: impl AsRef<Path>, skip: impl Fn(&Path, &Entry) -> bool) -> Vec<PathBuf> {
        self.record_value("root");
        let abs = self.abs(dir.as_ref());
        let mut files = Vec::new();
        let mut listed = Vec::new();
        self.reading.walk(&abs, &skip, &mut files, &mut listed);
        if listed.is_empty() {
            self.record(Question::List(self.reading.key(&abs)));
        }
        for d in &listed {
            self.record(Question::List(self.reading.key(d)));
        }
        files
    }

    /// Every file whose path relative to the root matches one of `patterns`, in name order,
    /// absolute. A pattern is anchored at the root and takes `*`, `?` and `**` as
    /// [`glob_match_anchored`](crate::glob_match_anchored) does. Only the directories under a
    /// pattern's literal leading part are walked.
    pub fn glob(&self, patterns: &[&str]) -> Vec<PathBuf> {
        self.record_value("root");
        let mut out: Vec<PathBuf> = Vec::new();
        for pattern in patterns {
            let literal: Vec<&str> = pattern
                .split('/')
                .take_while(|seg| !seg.contains(['*', '?', '[', '{']))
                .collect();
            let all_literal = literal.len() == pattern.split('/').count();
            if all_literal {
                if self.is_file(pattern) {
                    out.push(self.abs(Path::new(pattern)));
                }
                continue;
            }
            let base = self.abs(Path::new(&literal.join("/")));
            for f in self.walk(&base, |_, _| false) {
                let key = self.reading.key(&f);
                if crate::glob_match_anchored(pattern, &key) {
                    out.push(f);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// A crate's Rust files under `src/`, with every directory walked to find them written down,
    /// so a file appearing or going reruns the unit.
    pub(crate) fn crate_files(&self, crate_dir: &Path) -> &'r CrateFiles {
        let found = self.reading.crate_files(crate_dir);
        for d in &found.listed {
            self.record(Question::List(self.reading.key(d)));
        }
        if found.listed.is_empty() {
            self.record(Question::List(self.reading.key(&crate_dir.join("src"))));
        }
        found
    }

    fn kind(&self, path: &Path) -> Option<Kind> {
        let abs = self.abs(path);
        self.record(Question::Kind(self.reading.key(&abs)));
        self.reading.kind(&abs)
    }

    /// Whether anything is at `path`.
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.kind(path.as_ref()).is_some()
    }

    /// Whether `path` is a file, a symbolic link to one included.
    pub fn is_file(&self, path: impl AsRef<Path>) -> bool {
        self.kind(path.as_ref()) == Some(Kind::File)
    }

    /// Whether `path` is a directory, a symbolic link to one included.
    pub fn is_dir(&self, path: impl AsRef<Path>) -> bool {
        self.kind(path.as_ref()) == Some(Kind::Dir)
    }

    fn file(&self, path: &Path) -> PathBuf {
        let abs = self.abs(path);
        self.record(Question::File(self.reading.key(&abs)));
        abs
    }

    /// A file's bytes, `None` where it cannot be read.
    pub fn bytes(&self, path: impl AsRef<Path>) -> Option<&'r [u8]> {
        let abs = self.file(path.as_ref());
        self.reading.bytes(&abs)
    }

    /// A file's text, `None` where it cannot be read or is not UTF-8.
    pub fn text(&self, path: impl AsRef<Path>) -> Option<&'r str> {
        let abs = self.file(path.as_ref());
        self.reading.text(&abs)
    }

    /// A Rust file's syntax tree, parsed once for the run, `None` where it cannot be read.
    pub fn rust(&self, path: impl AsRef<Path>) -> Option<&'r tree_sitter::Tree> {
        let abs = self.file(path.as_ref());
        self.reading.tree(&abs)
    }

    /// A TOML file, parsed once for the run: `None` where it cannot be read, the parser's message
    /// where it is not TOML.
    pub fn toml(&self, path: impl AsRef<Path>) -> Option<Result<&'r toml::Table, &'r str>> {
        let abs = self.file(path.as_ref());
        self.reading.toml(&abs).map(|r| r.as_ref().map_err(String::as_str))
    }

    /// A syntax tree of `text`, which is no file: a snippet out of a document, a string a lint
    /// built. Parsed by this thread's parser every time it is asked, and nothing is written down,
    /// since the text came from something that was.
    pub fn parse(&self, text: &str) -> tree_sitter::Tree {
        self.reading.parse_text(text)
    }

    // -----------------------------------------------------------------------------------------
    // Processes
    // -----------------------------------------------------------------------------------------

    /// The answer of a read-only `git` invocation run at the root, asked once for the run.
    pub fn git(&self, args: &[&str]) -> &'r Answer {
        self.git_in(self.reading.root(), args)
    }

    /// The answer of a read-only `git` invocation run in `dir`, asked once for the run. A trace
    /// holding it is checked by asking it again, once for every lint that holds it.
    pub fn git_in(&self, dir: impl AsRef<Path>, args: &[&str]) -> &'r Answer {
        let cwd = self.reading.key(dir.as_ref());
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        self.record(Question::Git {
            cwd:  cwd.clone(),
            args: args.clone(),
        });
        self.reading.git_answer(&cwd, &args)
    }

    /// The output of `program` with `args`, run in `cwd`, once for the run. `inputs` are the files
    /// the output depends on, and they are what answers for it later: the command is never run
    /// again to check a trace, so a dependency left out of `inputs` replays a stale finding. The
    /// repository root is in the trace too, since the program is started under it, so a unit that
    /// ran a program is not replayed in another checkout.
    ///
    /// Its exit status is part of the answer. A command that exited non-zero is saved with the
    /// unit and replayed until its inputs move, the same as one that exited zero, so a failure that
    /// is the repository's (a package nobody takes, a stale lockfile) costs nothing on the next run.
    /// A command that could not be started, or that a signal ended, answered nothing and keeps its
    /// unit from being saved. Where a failed exit is the machine's and not the repository's, a
    /// network away or a tool too old, the lint says so with [`Self::untraced`] once it has seen
    /// it, and the unit is judged again on the next run.
    pub fn run(&self, program: &str, args: &[&str], cwd: impl AsRef<Path>, inputs: &[&Path]) -> &'r Answer {
        self.run_inner(program, args, cwd.as_ref(), None, inputs)
    }

    /// [`Self::run`], with `stdin` written to the program's input. The text is part of what the
    /// output is known by.
    pub fn run_with_input(
        &self,
        program: &str,
        args: &[&str],
        cwd: impl AsRef<Path>,
        stdin: &[u8],
        inputs: &[&Path],
    ) -> &'r Answer {
        self.run_inner(program, args, cwd.as_ref(), Some(stdin), inputs)
    }

    fn run_inner(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        stdin: Option<&[u8]>,
        inputs: &[&Path],
    ) -> &'r Answer {
        // The program is started in an absolute directory whatever name the trace gives it, and what
        // it prints can carry that directory, so the root is in the trace as `root()` puts it there.
        self.record_value("root");
        let cwd = self.reading.key(cwd);
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        let stdin_key = stdin.map(stdin_digest);
        // The inputs are read before the command can run, and their digests are taken from these
        // bytes when the unit is saved, so an input written while the command runs is saved under
        // what the command was started over and not under what it left behind.
        for p in inputs {
            let _ = self.reading.read(&self.abs(p));
        }
        let inputs: Vec<String> = inputs.iter().map(|p| self.reading.key(p)).collect();
        self.record(Question::Run {
            program: program.to_string(),
            args:    args.clone(),
            cwd:     cwd.clone(),
            stdin:   stdin_key.clone(),
            inputs,
        });
        self.reading.run_answer(program, &args, &cwd, stdin_key.as_deref().zip(stdin))
    }
}

/// What a command's input is known by in a trace: two independent 64-bit SipHash passes over the
/// bytes, which tell texts apart for a cache's purpose without the crate carrying a hash of its own
/// choosing.
fn stdin_digest(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut out = String::with_capacity(32);
    for salt in [0u8, 1u8] {
        let mut h = std::hash::DefaultHasher::new();
        salt.hash(&mut h);
        bytes.hash(&mut h);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}
