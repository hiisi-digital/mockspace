//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The store saved lint verdicts live in.
//!
//! **A verdict is saved under the digest of everything it depends on**, which the lint runners
//! assemble (see `mockspace_lint_rules::verdict`) and this module hashes. The digest is salted with
//! the identity of the code that produced the verdict: the engine binary and the library each
//! loaded pack came from, hashed by content. A rebuilt lint therefore finds nothing saved by the
//! one before it, and no version number has to be remembered to be bumped.
//!
//! **The store is local and never committed.** It is content-addressed, so a stale entry is not
//! wrong, only unreachable: nothing keys on it any more. That is why it can live outside the
//! worktree and be one store for the machine, shared by every checkout and every repository (the
//! digest holds the content, so two repositories can only share an entry when the answer is the
//! same), and why deleting it costs one full run and nothing else. The daily sweep covers the
//! whole of it. (`mock test`'s history is committed because a fresh container must not
//! rerun a suite; a lint run is cheap enough cold that a committed record is not worth the merge
//! traffic it would cause.)
//!
//! `MOCKSPACE_NO_VERDICTS=1` turns the store off for a run, which is how an incremental run is
//! compared with a full one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mockspace_lint_rules::verdict::VerdictCache;
use sha2::{Digest, Sha256};

/// The variable that turns the store off for a run.
pub const OFF: &str = "MOCKSPACE_NO_VERDICTS";

/// The variable that moves the store, for a machine whose cache directory is not the default and
/// for the tests, which keep theirs inside a directory they delete.
pub const DIR: &str = "MOCKSPACE_VERDICTS_DIR";

/// Entries untouched this long are removed by the sweep.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 60 * 60);

/// The store, salted for one engine and one set of packs.
pub struct DiskVerdicts {
    dir:  PathBuf,
    salt: [u8; 32],
}

impl DiskVerdicts {
    /// The store, or `None` when the run is not to use one: switched off, or nowhere to put it.
    /// `pack_identity` is empty when no pack is loaded.
    #[must_use]
    pub fn open(pack_identity: &str) -> Option<Self> {
        if std::env::var_os(OFF).is_some_and(|v| !v.is_empty() && v != "0") {
            return None;
        }
        Self::open_in(&base_dir()?, pack_identity)
    }

    /// [`Self::open`] against a store at `base`, for a caller that owns where it goes. The tests use
    /// it to keep their entries inside a directory they delete.
    #[must_use]
    pub fn open_in(base: &Path, pack_identity: &str) -> Option<Self> {
        let dir = base.to_path_buf();
        fs::create_dir_all(&dir).ok()?;

        let mut salt = Sha256::new();
        salt.update(b"verdict-store-1\0");
        salt.update(engine_identity()?.as_bytes());
        salt.update(b"\0");
        salt.update(pack_identity.as_bytes());
        let store = Self {
            dir,
            salt: salt.finalize().into(),
        };
        store.sweep();
        Some(store)
    }

    fn path_of(&self, key: &str) -> PathBuf {
        self.dir.join(&key[.. 2]).join(&key[2 ..])
    }

    /// Remove entries nothing has read in [`STALE_AFTER`], at most once a day.
    fn sweep(&self) {
        let marker = self.dir.join(".swept");
        let due = fs::metadata(&marker)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age > std::time::Duration::from_secs(24 * 60 * 60));
        if !due {
            return;
        }
        let _ = fs::write(&marker, b"");
        let Ok(shards) = fs::read_dir(&self.dir) else { return };
        // Only what this store wrote: a two-hex shard directory, and in it an entry named by the
        // rest of a key or one of its own half-written temporaries. The directory may be one the
        // user pointed `MOCKSPACE_VERDICTS_DIR` at, with other things in it.
        let is_hex = |s: &std::ffi::OsStr, len: usize| {
            s.to_str().is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        for shard in shards.flatten().filter(|s| s.path().is_dir() && is_hex(&s.file_name(), 2)) {
            let Ok(entries) = fs::read_dir(shard.path()) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let ours = is_hex(&name, 62)
                    || name
                        .to_str()
                        .and_then(|n| n.strip_prefix('.')?.split_once('.'))
                        .is_some_and(|(pid, key)| {
                            pid.bytes().all(|b| b.is_ascii_digit()) && is_hex(key.as_ref(), 62)
                        });
                if !ours {
                    continue;
                }
                let stale = entry
                    .metadata()
                    .and_then(|m| m.accessed().or_else(|_| m.modified()))
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > STALE_AFTER);
                if stale {
                    let _ = fs::remove_file(entry.path());
                }
            }
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
        fs::read_to_string(self.path_of(key)).ok()
    }

    fn tally(&self, hit: bool, lint: &str) {
        mockspace_lint_rules::timing::count(|| format!("{}:{lint}", if hit { "hit" } else { "miss" }));
    }

    fn remember(&self, key: &str, encoded: &str) {
        let path = self.path_of(key);
        let Some(shard) = path.parent() else { return };
        if fs::create_dir_all(shard).is_err() {
            return;
        }
        // Written beside and renamed over, so a reader never sees half an entry and two runs
        // saving the same key leave one whole file. The name carries the process, so they do
        // not write the same temporary.
        let tmp = shard.join(format!(".{}.{}", std::process::id(), &key[2 ..]));
        if fs::write(&tmp, encoded).is_ok() && fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
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

#[cfg(test)]
#[path = "verdicts_tests.rs"]
mod tests;
