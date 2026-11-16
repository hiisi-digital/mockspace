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
//! worktree and why deleting it costs one full run and nothing else. It is one packed file per
//! repository and salt, so a repository holds exactly one file of verdicts, and the first write
//! under a new salt (an engine or pack change) removes that repository's files of the old
//! salts. A packed file untouched for fourteen days is swept. (`mock test`'s history is committed
//! because a fresh container must not rerun a suite; a lint run is cheap enough cold that a
//! committed record is not worth the merge traffic it would cause.)
//!
//! `MOCKSPACE_NO_VERDICTS=1` turns the store off for a run, which is how an incremental run is
//! compared with a full one.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use mockspace_lint_rules::verdict::VerdictCache;
use sha2::{Digest, Sha256};

/// The variable that turns the store off for a run.
pub const OFF: &str = "MOCKSPACE_NO_VERDICTS";

/// The variable that moves the store, for a machine whose cache directory is not the default and
/// for the tests, which keep theirs inside a directory they delete.
pub const DIR: &str = "MOCKSPACE_VERDICTS_DIR";

/// A packed file untouched this long is removed by the sweep.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 60 * 60);

/// The extension of a packed file.
const PACK_EXT: &str = "pack";

/// The store, salted for one engine and one set of packs, kept for one repository.
///
/// **One packed file per (repository, salt)**: `<base>/<repository digest>/<salt digest>.pack`,
/// a run of records `<key> <length>\n<value>\n`. The file is read whole when the store opens and
/// appended to under an exclusive lock on the file (`File::lock`), one whole record per append, so
/// two `cargo mock` runs never interleave records. A record cut short by a crash is dropped, and
/// the file truncated back to its last whole record the next time a store opens.
pub struct DiskVerdicts {
    /// The repository's directory in the store.
    dir:     PathBuf,
    /// This salt's packed file.
    pack:    PathBuf,
    salt:    [u8; 32],
    entries: Mutex<HashMap<String, String>>,
    /// Whether the other salts of this repository have been removed by this store yet.
    pruned:  AtomicBool,
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
    /// Opening removes what the older one-file-per-verdict layout left in `base` (two-hex shard
    /// directories, the kept full-run keys). It is dropped and not migrated: a verdict is only a
    /// saved answer, and the next run makes it again.
    #[must_use]
    pub fn open_in(base: &Path, repo: &Path, pack_identity: &str) -> Option<Self> {
        fs::create_dir_all(base).ok()?;
        remove_legacy_layout(base);
        sweep_stale_packs(base);

        let dir = base.join(&hex(&Sha256::digest(repo.as_os_str().as_encoded_bytes()))[.. 16]);
        fs::create_dir_all(&dir).ok()?;

        let mut salt = Sha256::new();
        salt.update(b"verdict-store-2\0");
        salt.update(engine_identity()?.as_bytes());
        salt.update(b"\0");
        salt.update(pack_identity.as_bytes());
        let salt: [u8; 32] = salt.finalize().into();
        let pack = dir.join(format!("{}.{PACK_EXT}", hex(&salt)));
        let entries = load(&pack);
        Some(Self {
            dir,
            pack,
            salt,
            entries: Mutex::new(entries),
            pruned: AtomicBool::new(false),
        })
    }

    /// The directory the store keeps this repository's packed file in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Remove every packed file of this repository but this salt's: an engine or pack change
    /// starts a new salt, and nothing keys on the old one again.
    fn prune_other_salts(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p != self.pack && p.extension().is_some_and(|x| x == PACK_EXT) {
                let _ = fs::remove_file(p);
            }
        }
    }
}

/// The whole records of a packed file, and how many bytes they cover.
fn parse(bytes: &[u8]) -> (HashMap<String, String>, usize) {
    let mut out = HashMap::new();
    let mut at = 0;
    while let Some(nl) = bytes[at ..].iter().position(|&b| b == b'\n') {
        let Some((key, len)) = std::str::from_utf8(&bytes[at .. at + nl])
            .ok()
            .and_then(|h| h.split_once(' '))
            .and_then(|(k, l)| Some((k, l.parse::<usize>().ok()?)))
        else {
            break;
        };
        let body = at + nl + 1;
        let end = body + len;
        if bytes.get(end) != Some(&b'\n') {
            break;
        }
        let Ok(value) = std::str::from_utf8(&bytes[body .. end]) else { break };
        out.insert(key.to_string(), value.to_string());
        at = end + 1;
    }
    (out, at)
}

/// The entries of a packed file, truncating a torn tail away under the lock.
fn load(pack: &Path) -> HashMap<String, String> {
    let Ok(bytes) = fs::read(pack) else { return HashMap::new() };
    let (entries, valid) = parse(&bytes);
    if valid == bytes.len() {
        return entries;
    }
    // A cut-short record. Under the lock no writer is mid-append, so what is read now is final.
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(pack) else { return entries };
    if file.lock().is_err() {
        return entries;
    }
    let again = fs::read(pack).unwrap_or_default();
    let (entries, valid) = parse(&again);
    if valid < again.len() {
        let _ = file.set_len(valid as u64);
    }
    entries
}

/// Remove what the one-file-per-verdict layout left in `base`, and nothing else: a two-hex shard
/// directory's entries (a 62-hex key or its half-written temporary), the shard once empty, the
/// daily marker and the kept full-run keys. The directory may be one the user pointed
/// `MOCKSPACE_VERDICTS_DIR` at, with other things in it.
fn remove_legacy_layout(base: &Path) {
    let is_hex = |s: &std::ffi::OsStr, len: usize| {
        s.to_str().is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let _ = fs::remove_dir_all(base.join("last-full-run"));
    let _ = fs::remove_file(base.join(".swept"));
    let Ok(shards) = fs::read_dir(base) else { return };
    for shard in shards.flatten().filter(|s| s.path().is_dir() && is_hex(&s.file_name(), 2)) {
        let Ok(entries) = fs::read_dir(shard.path()) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let ours = is_hex(&name, 62)
                || name.to_str().and_then(|n| n.strip_prefix('.')?.split_once('.')).is_some_and(|(pid, key)| {
                    pid.bytes().all(|b| b.is_ascii_digit()) && is_hex(key.as_ref(), 62)
                });
            if ours {
                let _ = fs::remove_file(entry.path());
            }
        }
        // Only succeeds when empty, so a shard holding something of the user's stays.
        let _ = fs::remove_dir(shard.path());
    }
}

/// Remove packed files nothing has written or read in [`STALE_AFTER`], in every repository's
/// directory, and a repository directory left empty.
fn sweep_stale_packs(base: &Path) {
    let Ok(repos) = fs::read_dir(base) else { return };
    for repo in repos.flatten().filter(|r| r.path().is_dir()) {
        let ours = repo.file_name().to_str().is_some_and(|n| n.len() == 16 && n.bytes().all(|b| b.is_ascii_hexdigit()));
        if !ours {
            continue;
        }
        let Ok(packs) = fs::read_dir(repo.path()) else { continue };
        for p in packs.flatten().filter(|p| p.path().extension().is_some_and(|x| x == PACK_EXT)) {
            let stale = p
                .metadata()
                .and_then(|m| m.accessed().or_else(|_| m.modified()))
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STALE_AFTER);
            if stale {
                let _ = fs::remove_file(p.path());
            }
        }
        let _ = fs::remove_dir(repo.path());
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
        self.entries.lock().ok()?.get(key).cloned()
    }

    fn tally(&self, hit: bool, lint: &str) {
        mockspace_lint_rules::timing::count(|| format!("{}:{lint}", if hit { "hit" } else { "miss" }));
    }

    fn remember(&self, key: &str, encoded: &str) {
        let Ok(mut entries) = self.entries.lock() else { return };
        if entries.get(key).is_some_and(|v| v == encoded) {
            return;
        }
        let record = format!("{key} {}\n{encoded}\n", encoded.len());
        let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&self.pack) else { return };
        // One whole record per append, under the file's exclusive lock: two runs never interleave.
        if file.lock().is_err() || file.write_all(record.as_bytes()).is_err() {
            return;
        }
        drop(file);
        entries.insert(key.to_string(), encoded.to_string());
        if !self.pruned.swap(true, Ordering::Relaxed) {
            self.prune_other_salts();
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

#[cfg(all(test, unix))]
#[path = "verdicts_process_tests.rs"]
mod process_tests;
