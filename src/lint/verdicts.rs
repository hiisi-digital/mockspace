//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The store saved lint verdicts live in.
//!
//! **A verdict is saved under the digest of everything it depends on**, which the lint runners
//! assemble (see `mockspace_lint_rules::verdict`) and this module hashes. The digest is salted with
//! the identity of the code that produced the verdict: the engine binary by its bytes, and each
//! loaded pack by a digest of the inputs it was built from (`custom_lints::identity`), so one
//! commit has one identity from any tree. A changed lint therefore finds nothing saved by the
//! one before it, and no version number has to be remembered to be bumped.
//!
//! **The store is local and never committed.** It is content-addressed, so a stale entry is not
//! wrong, only unreachable: nothing keys on it any more. That is why it can live outside the
//! worktree and why deleting it costs one full run and nothing else. (`mock test`'s history is
//! committed because a fresh container must not rerun a suite; a lint run is cheap enough cold
//! that a committed record is not worth the merge traffic it would cause.)
//!
//! **A repository's verdicts under one salt are packed into one file per generation**, a
//! generation being [`KEPT`] of wall-clock time: `<base>/<repository>/<salt>.<generation>.pack`.
//! A store reads this generation's file and the one before, writes to this generation's, and
//! copies a verdict it replays from the one before into this one. So a verdict lasts at least
//! [`KEPT`] after it was last read or written and at most twice that, and what a store loads is
//! what was in use over the last two generations, each verdict at most twice. The files of an
//! older generation, of any salt, nothing reads any more, and opening a store removes them;
//! another salt in use keeps its two.
//!
//! **A record is framed and checked**: a magic, the key's and the value's lengths, a checksum of
//! all of them, then the key and the value. A record is taken only when its checksum holds, and a
//! reader that meets one that does not looks for the next magic, so a record cut short by a
//! process dying part way, or a value made of a cut record and the next one, is never replayed
//! and never hides the records after it. A write takes the file's exclusive lock and cuts the file
//! back to where it was when the write fails part way, so a store that keeps running after a
//! failed write leaves nothing of it behind.
//!
//! The older layout of one file per verdict, which released engines still write into the same
//! directory, and anything else this store wrote before, is removed only once both its access
//! and its modification time are [`KEPT`] old, so an engine still in use keeps its own.
//!
//! `MOCKSPACE_NO_VERDICTS=1` turns the store off for a run, which is how an incremental run is
//! compared with a full one.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mockspace_lint_rules::verdict::VerdictCache;
use sha2::{Digest, Sha256};

#[path = "verdicts_record.rs"]
mod record;
use record::{encode, parse};

/// The variable that turns the store off for a run.
pub const OFF: &str = "MOCKSPACE_NO_VERDICTS";

/// The variable that moves the store, for a machine whose cache directory is not the default and
/// for the tests, which keep theirs inside a directory they delete.
pub const DIR: &str = "MOCKSPACE_VERDICTS_DIR";

/// How long a verdict nobody reads or writes is kept at the least, and the length of a
/// generation. Anything else the store finds is removed once untouched this long.
const KEPT: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// How often the older layout is walked, since it can hold a hundred thousand files.
const LEGACY_SWEEP_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The extension of a packed file.
const PACK_EXT: &str = "pack";

/// The store, salted for one engine and one set of packs, kept for one repository.
///
/// Every verdict of the two generations it reads is held in memory from the open, so a replay
/// reads no file. Writes are appended one whole record at a time under the file's exclusive lock
/// (`File::lock`), so runs in several processes never interleave their records.
pub struct DiskVerdicts {
    /// The repository's directory in the store, made by the first write.
    dir:     PathBuf,
    /// This generation's packed file, the one written to.
    current: PathBuf,
    salt:    [u8; 32],
    state:   Mutex<State>,
}

struct State {
    entries: HashMap<String, Saved>,
    /// This generation's packed file, opened for appending by the first write.
    file:    Option<File>,
}

struct Saved {
    value:   String,
    /// Whether the value is in this generation's file, so a replay need not copy it there.
    current: bool,
}

impl DiskVerdicts {
    /// The store for the repository at `repo`, or `None` when the run is not to use one: switched
    /// off, or nowhere to put it. `pack_identity` is empty when no pack is loaded.
    #[must_use]
    pub fn open(repo: &Path, pack_identity: &str) -> Option<Self> {
        // A test gives the thread its own store, whatever the environment says.
        #[cfg(test)]
        if let Some(base) = test_base::get() {
            return Self::open_in(&base, repo, pack_identity);
        }
        if std::env::var_os(OFF).is_some_and(|v| !v.is_empty() && v != "0") {
            return None;
        }
        Self::open_in(&base_dir()?, repo, pack_identity)
    }

    /// [`Self::open`] against a store at `base`, for a caller that owns where it goes. The tests use
    /// it to keep their entries inside a directory they delete.
    ///
    /// Opening removes the packed files nothing reads any more, and once a day what of the older
    /// layout has been untouched for [`KEPT`].
    #[must_use]
    pub fn open_in(base: &Path, repo: &Path, pack_identity: &str) -> Option<Self> {
        Self::open_at(base, repo, pack_identity, SystemTime::now())
    }

    /// [`Self::open_in`] as it would open at `now`.
    fn open_at(base: &Path, repo: &Path, pack_identity: &str, now: SystemTime) -> Option<Self> {
        fs::create_dir_all(base).ok()?;
        let generation = now.duration_since(UNIX_EPOCH).ok()?.as_secs() / KEPT.as_secs();
        sweep(base, now, generation);

        let dir = base.join(&hex(&Sha256::digest(repo.as_os_str().as_encoded_bytes()))[.. 16]);
        let mut salt = Sha256::new();
        salt.update(b"verdict-store-3\0");
        salt.update(engine_identity()?.as_bytes());
        salt.update(b"\0");
        salt.update(pack_identity.as_bytes());
        let salt: [u8; 32] = salt.finalize().into();
        let file_of = |g: u64| dir.join(format!("{}.{g}.{PACK_EXT}", hex(&salt)));

        let mut entries = HashMap::new();
        // The generation before first, so this one's record of a key wins.
        for (g, current) in [(generation.checked_sub(1), false), (Some(generation), true)] {
            let Some(bytes) = g.and_then(|g| fs::read(file_of(g)).ok()) else { continue };
            for (key, value) in parse(&bytes) {
                entries.insert(key, Saved {
                    value,
                    current,
                });
            }
        }
        Some(Self {
            current: file_of(generation),
            dir,
            salt,
            state: Mutex::new(State {
                entries,
                file: None,
            }),
        })
    }

    /// Appends one record to this generation's file, and says whether all of it is there.
    fn append(&self, file: &mut Option<File>, key: &str, value: &str) -> bool {
        let Some(record) = encode(key, value) else { return false };
        if file.is_none() {
            *file = self.open_current();
        }
        file.as_ref().is_some_and(|f| append_whole(f, &record))
    }

    /// This generation's file, opened for appending and made when missing.
    fn open_current(&self) -> Option<File> {
        for _ in 0 .. 3 {
            match fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.current)
            {
                Ok(file) => return Some(file),
                // Not made yet, or another process's sweep removed it while it was still empty.
                Err(e) if e.kind() == ErrorKind::NotFound => fs::create_dir_all(&self.dir).ok()?,
                Err(_) => return None,
            }
        }
        None
    }
}

/// Appends `record` under the file's exclusive lock. A write that fails part way, a full disk or
/// a size limit, is cut back off, so the file is as it was; the lock is held throughout, so no
/// other writer's record is cut with it.
fn append_whole(file: &File, record: &[u8]) -> bool {
    if file.lock().is_err() {
        return false;
    }
    let whole = file.metadata().is_ok_and(|m| {
        let end = m.len();
        let ok = (&*file).write_all(record).is_ok();
        if !ok {
            let _ = file.set_len(end);
        }
        ok
    });
    let _ = file.unlock();
    whole
}

fn is_hex(s: &std::ffi::OsStr, len: usize) -> bool {
    s.to_str()
        .is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether both the access and the modification time of `path` are older than [`KEPT`] at
/// `now`. The later of the two counts, since a mount may keep no access times while a file is
/// rewritten, and a file may be read and never written.
fn untouched(path: &Path, now: SystemTime) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    let last = meta.accessed().map_or(modified, |a| a.max(modified));
    now.duration_since(last).is_ok_and(|age| age > KEPT)
}

/// Removes what nothing reads any more from the store at `base`, and nothing else: it may be a
/// directory the user pointed `MOCKSPACE_VERDICTS_DIR` at, with other things in it.
fn sweep(base: &Path, now: SystemTime, generation: u64) {
    let Ok(entries) = fs::read_dir(base) else { return };
    let mut legacy = Vec::new();
    for e in entries.flatten() {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = e.file_name();
        if is_hex(&name, 16) {
            sweep_repository(&e.path(), now, generation);
        } else if is_hex(&name, 2) || name == "last-full-run" {
            legacy.push(e.path());
        }
    }
    sweep_legacy(base, now, &legacy);
}

/// In one repository's directory: the packed files of a generation before the last two, of any
/// salt, and a packed file of no generation once untouched; then the directory, when empty.
fn sweep_repository(dir: &Path, now: SystemTime, generation: u64) {
    let Ok(files) = fs::read_dir(dir) else { return };
    let suffix = format!(".{PACK_EXT}");
    for f in files.flatten() {
        let name = f.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(&suffix)) else {
            continue;
        };
        let gone = match stem.split_once('.') {
            Some((salt, g)) if is_hex(salt.as_ref(), 64) => {
                g.parse::<u64>().is_ok_and(|g| g + 1 < generation)
            },
            None => is_hex(stem.as_ref(), 64) && untouched(&f.path(), now),
            Some(_) => false,
        };
        if gone {
            let _ = fs::remove_file(f.path());
        }
    }
    // Only succeeds when empty. A store that opened it and has not written yet makes it again.
    let _ = fs::remove_dir(dir);
}

/// The one-file-per-verdict layout, which released engines still write: in each two-hex shard an
/// entry named by the rest of a key or one of its half-written temporaries, and the kept full-run
/// keys, each once untouched; a directory of it once empty and unchanged for [`KEPT`]; and its
/// daily marker, which it shares, once untouched. Walked at most once a day.
fn sweep_legacy(base: &Path, now: SystemTime, dirs: &[PathBuf]) {
    let marker = base.join(".swept");
    if dirs.is_empty() {
        if untouched(&marker, now) {
            let _ = fs::remove_file(&marker);
        }
        return;
    }
    let due = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .map_or(true, |t| {
            now.duration_since(t)
                .map_or(true, |age| age > LEGACY_SWEEP_EVERY)
        });
    if !due {
        return;
    }
    let _ = File::create(&marker).and_then(|f| f.set_modified(now));
    for dir in dirs {
        let full_run = dir.file_name().is_some_and(|n| n == "last-full-run");
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let ours = full_run
                || is_hex(&name, 62)
                || name
                    .to_str()
                    .and_then(|n| n.strip_prefix('.')?.split_once('.'))
                    .is_some_and(|(pid, key)| {
                        pid.bytes().all(|b| b.is_ascii_digit()) && is_hex(key.as_ref(), 62)
                    });
            if ours && untouched(&entry.path(), now) {
                let _ = fs::remove_file(entry.path());
            }
        }
        // By its modification time alone, which a walk does not change and an entry made or
        // removed does; succeeds only when empty.
        let unchanged = fs::metadata(dir)
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > KEPT));
        if unchanged {
            let _ = fs::remove_dir(dir);
        }
    }
}

impl VerdictCache for DiskVerdicts {
    fn digest(&self, parts: &[&[u8]]) -> String {
        let mut h = Sha256::new();
        h.update(self.salt);
        for p in parts {
            h.update((p.len() as u64).to_le_bytes());
            h.update(p);
        }
        hex(&h.finalize())
    }

    fn replay(&self, key: &str) -> Option<String> {
        let mut state = self.state.lock().ok()?;
        let State {
            entries,
            file,
        } = &mut *state;
        let saved = entries.get_mut(key)?;
        // Read from the generation before: copied into this one, so it lasts another generation.
        if !saved.current && self.append(file, key, &saved.value) {
            saved.current = true;
        }
        Some(saved.value.clone())
    }

    fn tally(&self, hit: bool, lint: &str) {
        mockspace_lint_rules::timing::count(|| {
            format!("{}:{lint}", if hit { "hit" } else { "miss" })
        });
    }

    fn remember(&self, key: &str, encoded: &str) {
        let Ok(mut state) = self.state.lock() else { return };
        let State {
            entries,
            file,
        } = &mut *state;
        if entries
            .get(key)
            .is_some_and(|s| s.current && s.value == encoded)
        {
            return;
        }
        if self.append(file, key, encoded) {
            entries.insert(key.to_string(), Saved {
                value:   encoded.to_string(),
                current: true,
            });
        }
    }
}

/// The identity of this engine: the digest of the executable's bytes.
///
/// By content and not by version string, so a rebuild of the same version, which is every
/// development build, still finds nothing saved by the one before. Hashed once per process.
fn engine_identity() -> Option<&'static str> {
    static ID: OnceLock<Option<String>> = OnceLock::new();
    ID.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        file_digest(&exe)
    })
    .as_deref()
}

/// The digest of a file's bytes, hex, or `None` if it cannot be read.
///
/// A pack's identity is made with this by the code that loads it, so the two kinds of identity
/// are spelt the same way.
#[must_use]
pub fn file_digest(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[.. n]);
    }
    Some(hex(&h.finalize()))
}

fn base_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(DIR).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    // The XDG specification says a relative value is invalid and to be ignored.
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
                .filter(|p| p.is_absolute())
        })
        .map(|c| c.join("mockspace").join("verdicts"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The store a test thread was given.
#[cfg(test)]
pub(crate) mod test_base {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static BASE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    pub(crate) fn get() -> Option<PathBuf> {
        BASE.with(|b| b.borrow().clone())
    }

    /// Runs `f` with this thread's store at `base`.
    pub(crate) fn with<R>(base: PathBuf, f: impl FnOnce() -> R) -> R {
        BASE.with(|b| *b.borrow_mut() = Some(base));
        let out = f();
        BASE.with(|b| *b.borrow_mut() = None);
        out
    }
}

#[cfg(test)]
#[path = "verdicts_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "verdicts_process_tests.rs"]
mod process_tests;
