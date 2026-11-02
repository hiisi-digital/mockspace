//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The test history on a [`Store`]: a directory, as it has always been, or a git ref.
//!
//! The files are the ones [`super::history`] describes, one per member per flavour, named
//! `<flavour>/<member>.json` from the root of the store. [`History::load_from`] reads them from
//! any [`Reader`], and [`History::save_to`] writes a run's records as one edit of a store.
//!
//! **What the writer guarantees.** An edit names only the members the run covered and the members
//! the workspace no longer has, so a green record of a member the run did not cover is never in
//! it, and survives a lost race byte for byte. It is not the commit guard: `mock check-test-history`
//! also excuses a null when the member's fingerprint inputs were touched, which needs the
//! commit's changed paths, and a write to a ref has no commit hook. The commit guard stays for a
//! directory kept in the tree. A run cut short is held off by `complete` in `super` (the run module), ahead
//! of the save.

use mockspace_store::{Applied, Edit, MergeHook, MergeRefusal, Reader, Store, StoreError};

use super::history::{History, MemberFile, PREVIOUS, Scope, VERSION, flavour_dir, member_files};
use super::history_merge::Later;

/// Why a history could not be read or written.
#[derive(Debug)]
pub enum HistoryError {
    Store(StoreError),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for HistoryError {}

impl From<StoreError> for HistoryError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// The member a record file belongs to: its name without the extension.
fn member_of(path: &str) -> Option<&str> {
    path.rsplit('/').next()?.strip_suffix(".json")
}

fn record_at(reader: &dyn Reader, path: &str) -> Result<Option<MemberFile>, StoreError> {
    Ok(reader
        .read(path)?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| MemberFile::parse(&t).ok()))
}

impl History {
    /// The record, from every file of `reader`. A file that does not parse, or is of another
    /// version, is passed over, as it is on disk: a lost record costs one run of that member.
    /// A reader that cannot be read is an error and never an empty record.
    pub fn load_from(reader: &dyn Reader) -> Result<History, StoreError> {
        let mut h = History {
            version: VERSION,
            ..History::default()
        };
        for path in reader.list("")? {
            if path.split('/').count() != 2 || member_of(&path).is_none() {
                continue;
            }
            if let Some(m) =
                record_at(reader, &path)?.filter(|m| m.version == VERSION || m.version == PREVIOUS)
            {
                h.absorb(m);
            }
        }
        Ok(h)
    }

    fn absorb(&mut self, m: MemberFile) {
        let older = m.version == PREVIOUS;
        let f = self.flavour_mut(&m.flavour);
        // The fingerprints of an older record were made of other inputs, so no pass of it
        // stands. What a test took does not depend on them, and says which tests are heavy.
        if let Some(g) = m.green.filter(|_| !older) {
            f.green.insert(m.member.clone(), g);
        }
        for (bin, mut tests) in m.tests {
            if older {
                tests.values_mut().for_each(|t| t.passed_at = None);
            }
            f.tests.entry(bin).or_default().extend(tests);
        }
        f.bins.extend(m.bins);
        if !older {
            f.red.extend(m.red);
        }
    }

    /// What a save changes in `reader`: the files of the members the run covered whose text
    /// differs, and the files of members the run covered that now hold nothing, or that the
    /// workspace no longer has. Every other file stands.
    fn plan(&self, reader: &dyn Reader, scope: &Scope) -> Result<Edit, StoreError> {
        let dir = flavour_dir(scope.flavour);
        let wanted = self
            .flavours
            .get(scope.flavour)
            .map(|f| member_files(scope.flavour, f))
            .unwrap_or_default();
        let mut edit = Edit::new();
        for path in reader.list("")? {
            let Some((fdir, _)) = path.split_once('/') else { continue };
            let Some(member) = member_of(&path) else { continue };
            let doomed = if fdir == dir {
                scope.gone(member) || (scope.ran(member) && !wanted.contains_key(member))
            } else {
                scope.gone(member)
            };
            if doomed {
                edit = edit.remove(path);
            }
        }
        for (member, m) in wanted.into_iter().filter(|(member, _)| scope.ran(member)) {
            let path = format!("{dir}/{member}.json");
            let text = m.render();
            if reader
                .read(&path)?
                .is_some_and(|old| old == text.as_bytes())
            {
                continue;
            }
            edit = edit.put(path, text);
        }
        Ok(edit)
    }

    /// Writes a run's records to `store` as one edit with `message`, on the state the store
    /// last saw (a store is refreshed once, when it is opened). A lost race is settled file by file with [`super::history_merge`].
    pub fn save_to(
        &self,
        store: &dyn Store,
        scope: &Scope,
        message: &str,
    ) -> Result<Applied, HistoryError> {
        let edit = self.plan(store, scope)?;
        let ours = edit.clone();
        let base = ours
            .entries()
            .map(|(p, _)| Ok((p.to_string(), store.read(p)?)))
            .collect::<Result<Vec<_>, StoreError>>()?;
        let race = Race {
            base,
        };
        Ok(store.apply(&edit, message, &race)?)
    }
}

/// The race handler: the merge of the history files.
struct Race {
    /// The touched files as the write found them.
    base: Vec<(String, Option<Vec<u8>>)>,
}

impl MergeHook for Race {
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        let refuse = |why: String| MergeRefusal(why);
        let text = |b: Option<&[u8]>| {
            b.and_then(|b| std::str::from_utf8(b).ok())
                .map(String::from)
        };
        let mut next = Edit::new();
        for (path, bytes) in ours.entries() {
            let base = self
                .base
                .iter()
                .find(|(p, _)| p == path)
                .and_then(|(_, b)| text(b.as_deref()));
            let theirs = text(
                tip.read(path)
                    .map_err(|e| refuse(e.to_string()))?
                    .as_deref(),
            );
            next = match (text(bytes), theirs) {
                // our removal of a file the winner changed does not stand: its record is kept and the next run settles it
                (None, Some(t)) if base.as_deref() != Some(t.as_str()) => next,
                (None, _) => next.remove(path),
                (Some(o), Some(t)) if base.as_deref() != Some(t.as_str()) => {
                    let merged = super::history_merge::merge_text(
                        base.as_deref().unwrap_or(""),
                        &o,
                        &t,
                        Later::Unknown,
                    )
                    .map_err(|why| refuse(format!("{path}: {why}")))?;
                    next.put(path, merged.text)
                },
                (Some(o), _) => next.put(path, o),
            };
        }
        Ok(next)
    }
}

/// The store of a project's test history: the directory under `<mock>/test-history`, or the
/// git ref `[test.history]` declares.
pub fn store_of(cfg: &crate::config::Config) -> Result<Box<dyn Store>, String> {
    match &cfg.test.history {
        None => {
            Ok(Box::new(mockspace_store::DirStore::new(History::dir(
                &cfg.mock_dir,
            ))))
        },
        Some(decl) => Ok(Box::new(decl.open(&cfg.repo_root, "test-history")?)),
    }
}

impl History {
    /// The history of a project and the store it is kept in, for a run to save back to.
    /// A store that cannot be read gives no history and no store, with the reason printed:
    /// the run goes on without a record, and writes none, rather than starting from one
    /// that reads as empty.
    pub fn open(cfg: &crate::config::Config) -> (History, Option<Box<dyn Store>>) {
        let opened = store_of(cfg).and_then(|s| {
            s.refresh().map_err(|e| e.to_string())?;
            let h = History::load_from(s.as_ref()).map_err(|e| e.to_string())?;
            Ok((h, s))
        });
        match opened {
            Ok((h, s)) => (h, Some(s)),
            Err(e) => {
                eprintln!(
                    "note: the test history could not be read: {e}; every member runs, and nothing is cached or recorded"
                );
                (
                    History {
                        version: VERSION,
                        ..History::default()
                    },
                    None,
                )
            },
        }
    }
}

#[cfg(test)]
#[path = "history_store_tests.rs"]
mod tests;
