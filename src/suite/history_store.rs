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
//! **The writer carries the guard.** `mock check-test-history` refuses a commit that nulls a
//! member's green record while touching nothing its tests are built from, because a run cut short
//! or a flaky test leaves exactly that, and a write to a ref has no commit hook to ask. The
//! writer asks the same question of its own edit: a green may go from a file only when the
//! member's fingerprint now differs from the one the record was made at (its inputs moved), when
//! the workspace no longer has the member. The guard is asked again of the winner's state after a lost race, because the merge rule of
//! [`super::history_merge`] drops a green the winner moved where this run moved it to nothing,
//! which the commit guard accepts in a merge commit and a record on a ref does not. Anything else is refused before it is pushed,
//! and nothing is written. The guard runs for a history kept on a ref only; a directory in the
//! tree keeps the commit guard as its only place the question is asked. An edit names only the
//! members the run covered and those the workspace no longer has, so the record of any other
//! member stands untouched whichever way the guard goes.

use std::collections::BTreeMap;

use mockspace_store::{Applied, Edit, MergeHook, MergeRefusal, Reader, Store, StoreError};

use super::history::{History, MemberFile, PREVIOUS, Scope, VERSION, flavour_dir, member_files};
use super::history_merge::Later;

/// Why a history could not be read or written.
#[derive(Debug)]
pub enum HistoryError {
    Store(StoreError),
    /// The guard refused the write: each entry is a member whose green record the run would remove
    /// although nothing its tests are built from moved since it was recorded.
    Nulled(Vec<String>),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => write!(f, "{e}"),
            Self::Nulled(members) => {
                write!(
                    f,
                    "the run would remove the green record of {}, whose inputs have not moved: a run cut short or a flaky test leaves that, so nothing is written; run it again",
                    members.join(", ")
                )
            },
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
    /// last saw. `guard` is each member's fingerprint now, for a store the commit guard does not
    /// cover, and refuses an edit that removes a green recorded at the fingerprint a member still has (a store is refreshed once, when it is opened). A lost race is settled file by file with [`super::history_merge`].
    pub fn save_to(
        &self,
        store: &dyn Store,
        scope: &Scope,
        guard: Option<&BTreeMap<String, String>>,
        message: &str,
    ) -> Result<Applied, HistoryError> {
        let edit = self.plan(store, scope)?;
        if let Some(now) = guard {
            let nulled = nulls(store, &edit, now)?;
            if !nulled.is_empty() {
                return Err(HistoryError::Nulled(nulled));
            }
        }
        let ours = edit.clone();
        let base = ours
            .entries()
            .map(|(p, _)| Ok((p.to_string(), store.read(p)?)))
            .collect::<Result<Vec<_>, StoreError>>()?;
        let race = Race {
            base,
            guard: guard.cloned(),
        };
        Ok(store.apply(&edit, message, &race)?)
    }
}

impl History {
    /// Keeps a run in `store`, the way a run does: the guard of the writer is asked only for a
    /// history kept on a ref (`[test.history]`), because a directory in the tree has the commit
    /// guard and nothing else to ask.
    pub fn keep(
        &self,
        cfg: &crate::config::Config,
        store: &dyn Store,
        scope: &Scope,
        fp: &super::fingerprint::Fingerprints,
    ) -> Result<Applied, HistoryError> {
        self.save_to(
            store,
            scope,
            cfg.test.history.is_some().then_some(&fp.full),
            "test history",
        )
    }
}

/// The race handler: the merge of the history files.
struct Race {
    /// The touched files as the write found them.
    base:  Vec<(String, Option<Vec<u8>>)>,
    /// Each member's fingerprint now, where the writer is the guard.
    guard: Option<BTreeMap<String, String>>,
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
        // A write that passed the guard on the state it read can still remove a green once it is
        // rebased: the merge rule drops a green that the winner moved to the fingerprint this run
        // still has, when the run moved it to nothing. The guard is asked again, of the tip.
        if let Some(now) = &self.guard {
            let nulled = nulls(tip, &next, now).map_err(|e| refuse(e.to_string()))?;
            if !nulled.is_empty() {
                return Err(refuse(HistoryError::Nulled(nulled).to_string()));
            }
        }
        Ok(next)
    }
}

fn green_of(bytes: Option<&[u8]>) -> Option<super::history::Green> {
    bytes
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|t| MemberFile::parse(t).ok())
        .and_then(|m| m.green)
}

/// The members whose green record `edit` removes from `reader` although their fingerprint in `now`
/// is the one it was recorded at.
fn nulls(
    reader: &dyn Reader,
    edit: &Edit,
    now: &BTreeMap<String, String>,
) -> Result<Vec<String>, StoreError> {
    let mut out = Vec::new();
    for (path, bytes) in edit.entries() {
        let Some(member) = member_of(path) else { continue };
        let Some(had) = record_at(reader, path)?.and_then(|m| m.green) else {
            continue;
        };
        if green_of(bytes).is_none() && now.get(member) == Some(&had.full) {
            out.push(member.to_string());
        }
    }
    Ok(out)
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
        Some(decl) if decl.dir.is_some() => {
            Err("`dir` names a directory of the registry, and a test history has none".into())
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
