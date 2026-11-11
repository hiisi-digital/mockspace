//! How an instance's changes reach the ref, and how the directory follows the ref.

use std::collections::{BTreeMap, BTreeSet};

use super::Instance;
use super::tracking::Tracking;
use crate::edit::Edit;
use crate::error::StoreError;
use crate::ids::{Base, FileId, Oid};
use crate::merge::MergeHook;
use crate::ref_history::BATCH;
use crate::store::{Applied, Freshness, Store};

impl Instance<'_> {
    /// Applies what [`Instance::edit`] finds through the store as an edit built on the base,
    /// then brings the directory to the tip, which holds what the hook made of any file a
    /// writer who won meanwhile changed too. Nothing differing is a no-op that touches neither
    /// the store nor the remote. A hook that refuses leaves the directory and the base as they
    /// were, and the way on is another apply with a hook that merges. When the edit landed and
    /// the directory could not be moved after it (a file was changed during the call), the
    /// error is the move's and the edit stays landed.
    pub fn apply(&self, message: &str, hook: &dyn MergeHook) -> Result<Applied, StoreError> {
        let delta = self.delta()?;
        if delta.sent.is_empty() {
            return Ok(Applied {
                commit:    None,
                changed:   false,
                attempts:  0,
                confirmed: false,
            });
        }
        let tracked = Tracking::new(hook);
        // an instance of a ref not born yet was built on none: where another clone made the ref
        // meanwhile, the hook is asked, as for any edit built on an older tip
        let edit = self.edit_of(&delta)?;
        let built_on = delta.base.commit().map(Oid::as_str);
        let applied = self.store.apply_from(built_on, &edit, message, &tracked)?;
        // Of what was sent, the files the tip now settles: all of them when the hook was never
        // asked, else the ones it returned an entry for. A file it left out was not written.
        let mut settled = delta.sent;
        if let Some(kept) = tracked.kept() {
            settled.retain(|path, _| kept.contains(path));
        }
        self.follow(&settled)?;
        Ok(applied)
    }

    /// Makes the ref from a directory that holds the files and has no ref yet: every regular file
    /// of the directory is put onto an empty ref, and the directory is then an instance of it,
    /// its base the commit written. Refused where the directory has a record already (it is
    /// committed with [`Instance::apply`]), where the ref has a tip (it is instantiated into an
    /// empty directory instead), and where the directory holds no files.
    pub fn seed(&self, message: &str, hook: &dyn MergeHook) -> Result<Applied, StoreError> {
        self.seed_among(None, message, hook)
    }

    /// [`Instance::seed`] from the files of `only` alone. A file of the directory that is not among
    /// them is left where it is, and reads as added here against the ref that was made.
    pub fn seed_tracked(
        &self,
        only: &BTreeSet<String>,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        self.seed_among(Some(only), message, hook)
    }

    fn seed_among(
        &self,
        only: Option<&BTreeSet<String>>,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        if self.base()?.is_some() {
            return Err(self.fail("has a record already, so its changes go through apply", Vec::new()));
        }
        if self.store.local_tip()?.is_some() {
            return Err(self.fail("is not made from a ref that already has a tip, which is instantiated", Vec::new()));
        }
        let here: Vec<String> = self.walk()?.into_iter().filter(|p| only.is_none_or(|o| o.contains(p))).collect();
        if here.is_empty() {
            return Err(self.fail("holds no files to seed the ref with", Vec::new()));
        }
        let mut edit = Edit::new();
        let trust_mode = self.file_mode()?;
        for path in &here {
            let bytes = std::fs::read(self.dir.join(path)).map_err(|e| self.io("reading", e))?;
            edit = if trust_mode && self.is_exec(path) { edit.put_exec(path.clone(), bytes) } else { edit.put(path.clone(), bytes) };
        }
        // The exclusion goes first: it is the step that can fail on a read-only checkout, and once
        // the ref has a tip a failure after it leaves a directory that neither `seed` (the ref has a
        // tip) nor `apply` (no record) takes.
        self.exclude()?;
        let applied = self.store.apply_from(None, &edit, message, hook)?;
        let Some(tip) = applied.commit.clone() else {
            return Err(self.fail("was seeded and the ref has no commit", Vec::new()));
        };
        if let Err(e) = Oid::parse(&tip).and_then(|oid| self.record(&Base::At(oid))) {
            return Err(self.fail(
                "made the ref (tip and cause follow) but could not record the directory as its instance; remove its files and instantiate it",
                vec![tip, e.to_string()],
            ));
        }
        Ok(applied)
    }

    /// Whether the directory, which has no record, holds exactly the files of the ref's local
    /// tip: the same paths with the same contents. That is the state a [`Instance::seed`] leaves
    /// when it made the ref and was stopped before it recorded the directory. Writes nothing.
    pub fn is_the_tip(&self) -> Result<bool, StoreError> {
        if self.base()?.is_some() || !self.dir.is_dir() {
            return Ok(false);
        }
        let Some(tip) = self.store.local_oid()? else { return Ok(false) };
        let held = self.store.tree_ids(&tip)?;
        self.check_held(&held)?;
        let here = self.walk()?;
        Ok(self.local_ids(&here, &held, true)? == held)
    }

    /// Records the directory as an instance of the ref at its local tip, where
    /// [`Instance::is_the_tip`] holds, and refuses where it does not. No file is written, since
    /// the directory already holds what the tip does.
    pub fn adopt(&self) -> Result<String, StoreError> {
        if !self.is_the_tip()? {
            return Err(self.fail("does not hold exactly the files of the ref's tip, so it is not taken as its instance", Vec::new()));
        }
        let Some(tip) = self.store.local_oid()? else {
            return Err(self.fail("has no tip to be an instance of", Vec::new()));
        };
        self.exclude()?;
        self.record(&Base::At(tip.clone()))?;
        Ok(tip.to_string())
    }

    /// Fetches the ref and brings the directory to its tip. A file changed here and on the tip
    /// both is a conflict, and so is a file here where the tip has a directory or the other way
    /// round: the call writes nothing and names the files, and the directory keeps what it had.
    /// A file changed here only stays as it is and is still part of the next edit.
    pub fn update(&self) -> Result<Freshness, StoreError> {
        let fresh = self.store.refresh()?;
        self.follow(&BTreeMap::new())?;
        Ok(fresh)
    }

    /// Brings the directory from its base to the local tip. `sent` is what an apply just put or
    /// removed and the tip now settles, because the hook returned an entry for it or was never
    /// asked: a file that still is what was sent is taken whatever the tip holds.
    pub(super) fn follow(&self, sent: &BTreeMap<String, Option<FileId>>) -> Result<(), StoreError> {
        let Some(tip) = self.store.local_oid()? else { return Ok(()) };
        let base = self.require_base()?;
        if base.commit() == Some(&tip) {
            return Ok(());
        }
        let (from, to) = (self.ids_of(&base)?, self.store.tree_ids(&tip)?);
        self.check_held(&to)?;
        let moved: Vec<String> = from
            .keys()
            .chain(to.keys())
            .chain(sent.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|p| from.get(*p) != to.get(*p) || sent.contains_key(*p))
            .cloned()
            .collect();
        // where the mode of a file is not read off the disk it is the one the base holds, and for a
        // path the base does not hold the one of the tip
        let mut modes = from.clone();
        for (path, id) in &to {
            modes.entry(path.clone()).or_insert_with(|| id.clone());
        }
        let here = self.local_ids(&moved, &modes, false)?;
        let mut plan = Vec::new();
        let mut conflicts = Vec::new();
        for path in &moved {
            let local = here.get(path);
            if local == to.get(path) {
                continue;
            }
            let as_sent = sent.get(path).is_some_and(|s| s.as_ref() == local);
            if local == from.get(path) || as_sent {
                plan.push(path);
            } else {
                conflicts.push(path.clone());
            }
        }
        if !conflicts.is_empty() {
            return Err(self.fail("holds files changed here and on the tip", conflicts));
        }
        let removals: Vec<&String> = plan
            .iter()
            .copied()
            .filter(|p| !to.contains_key(*p))
            .collect();
        let writes: Vec<&String> = plan
            .iter()
            .copied()
            .filter(|p| to.contains_key(*p))
            .collect();
        self.check_parents(&plan)?;
        let in_the_way = self.in_the_way(&removals, &writes)?;
        if !in_the_way.is_empty() {
            return Err(self.fail("holds files in the way of the tip", in_the_way));
        }
        for path in removals {
            match std::fs::remove_file(self.dir.join(path)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(self.io("removing", e));
                },
                _ => self.prune(path),
            }
        }
        for chunk in writes.chunks(BATCH) {
            let wanted: Vec<&Oid> = chunk.iter().map(|p| &to[*p].blob).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                // a directory at a path to be written holds directories only, or `in_the_way` refused
                if self.dir.join(path.as_str()).is_dir() {
                    std::fs::remove_dir_all(self.dir.join(path.as_str()))
                        .map_err(|e| self.io("removing", e))?;
                }
                self.write(path, &bytes, to[*path].mode.is_exec())?;
            }
        }
        self.record(&Base::At(tip))
    }

}
