//! A directory that holds the files of a ref's tip, for the tools that read files.
//!
//! An instance is a view of a ref, not a checkout of it: nothing is staged, no index file is
//! made, and git's ignore rules do not apply, so every regular file in the directory is data.
//! What the instance remembers is its base, the commit its files were last brought to, and that
//! is kept in the repository's own directory, apart from the files. A change is the difference
//! between the directory and the base, taken as an [`Edit`] (a put for each file added or
//! changed, a remove for each file gone), and it is applied through the store as an edit built
//! on the base. A file no other writer touched since the base is simply written. A file the
//! tip changed too goes through the merge hook, which sees the tip and the edit, and reads the
//! base through [`RefStore::reader_of`] and [`Instance::base`] when it merges three ways. For
//! each file the hook returns an entry for, what it returns is what the tip holds, and the
//! directory takes it; a hook that means to keep the tip's file returns a put of it. A file of
//! the edit that the hook leaves out is not written, and stays as it is here, to go with the
//! next edit or to be named as a conflict where the tip changed it too.
//!
//! The directory is excluded from the status of the branch checked out beside it, through the
//! repository's shared exclude file. Changes in an instance exist nowhere else until
//! [`Instance::apply`] runs, and `git clean -fdx` removes an excluded directory with them.
//! Nothing here guards a file saved by another process in the instant between the comparison
//! and the write of [`Instance::update`]; an instance is not edited while that runs.

mod files;
mod held;
mod records;
mod tracking;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub use held::Held;
use tracking::Tracking;

use crate::edit::Edit;
use crate::error::StoreError;
use crate::merge::MergeHook;
use crate::ref_history::BATCH;
use crate::ref_store::RefStore;
use crate::store::{Applied, Freshness, Store};

/// A directory kept in step with the local ref of a [`RefStore`].
pub struct Instance<'a> {
    store: &'a RefStore,
    dir:   PathBuf,
    held:  Option<Held>,
}

/// What the directory held against the base: the edit, the base it is against, and the blob id
/// of every file the edit puts (`None` for one it removes), which says afterwards whether a
/// file still is what was sent.
struct Delta {
    base: String,
    edit: Edit,
    sent: BTreeMap<String, Option<String>>,
}

impl<'a> Instance<'a> {
    /// An instance in `dir`, which is made absolute with the part that exists resolved through
    /// symlinks and `..`, so that two spellings of one directory are one instance. Nothing is
    /// read or written yet.
    pub fn new(store: &'a RefStore, dir: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let given = dir.into();
        let absolute = std::path::absolute(&given).map_err(|source| {
            StoreError::Io {
                context: format!("making `{}` absolute", given.display()),
                source,
            }
        })?;
        let this = Self {
            store,
            dir: resolved(&absolute),
            held: None,
        };
        match this.dir.to_str() {
            Some(text) if !text.chars().any(char::is_control) => Ok(this),
            _ => Err(this.fail("a path that is not plain text", Vec::new())),
        }
    }

    /// The same instance holding only the files `held` matches. The directory may then hold
    /// files of the branch as well: they are not read, written, removed or excluded, and a
    /// tip that holds a file the patterns do not match is refused before anything is written.
    pub fn holding(mut self, held: Held) -> Self {
        self.held = Some(held);
        self
    }

    /// The directory, absolute.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn holds(&self, rel: &str) -> bool {
        self.held.as_ref().is_none_or(|h| h.holds(rel))
    }

    /// Refuses a tree of the ref that holds a path this instance does not: written here it
    /// would be a file the branch sees and no later edit of the instance could name.
    fn check_held(&self, ids: &BTreeMap<String, String>) -> Result<(), StoreError> {
        let outside: Vec<String> = ids.keys().filter(|p| !self.holds(p)).cloned().collect();
        if outside.is_empty() {
            Ok(())
        } else {
            Err(self.fail("is of a ref that holds files outside its patterns", outside))
        }
    }

    /// The commit the files of the directory were last brought to, or `None` where no instance
    /// was made here.
    pub fn base(&self) -> Result<Option<String>, StoreError> {
        self.read_record()
    }

    /// Writes the files of the local tip into the directory, which must be absent or empty, and
    /// records the tip as the base. Gives the tip. An empty directory that has a record, whose
    /// base holds files, is one where every file was removed, and is refused: the removals are
    /// an edit not yet applied, and the directory itself removed starts the instance again.
    /// When it fails after writing, the files it wrote and the record are removed again, and a
    /// directory it made with them.
    pub fn materialize(&self) -> Result<String, StoreError> {
        let Some(tip) = self.store.local_tip()? else {
            return Err(StoreError::NotFetched {
                local_ref: self.store.pair().local().to_owned(),
            });
        };
        let existed = self.dir.exists();
        if existed {
            if self.held.is_some() {
                let here = self.walk()?;
                if !here.is_empty() {
                    return Err(self.fail("already holds files its patterns match", here));
                }
            } else {
                let mut entries = std::fs::read_dir(&self.dir).map_err(|e| self.io("reading", e))?;
                if entries.next().is_some() {
                    return Err(self.fail("is not an empty directory", Vec::new()));
                }
            }
            if let Some(base) = self.base()? {
                if !self.store.tree_ids(&base)?.is_empty() {
                    return Err(self.fail(
                        "has a record and no files, so every file was removed here",
                        Vec::new(),
                    ));
                }
            }
        }
        let ids = self.store.tree_ids(&tip)?;
        self.check_held(&ids)?;
        self.check_parents(&ids.keys().collect::<Vec<_>>())?;
        let made = self
            .fill(&tip)
            .and_then(|()| self.exclude())
            .and_then(|()| self.record(&tip));
        if made.is_err() {
            self.forget();
            if self.held.is_some() {
                // the directory holds the branch's files too, so only what the fill wrote goes
                for path in ids.keys() {
                    if std::fs::remove_file(self.dir.join(path)).is_ok() {
                        self.prune(path);
                    }
                }
                if !existed {
                    let _ = std::fs::remove_dir(&self.dir);
                }
            } else if existed {
                for entry in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(path)
                    } else {
                        std::fs::remove_file(path)
                    };
                }
            } else {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }
        made?;
        Ok(tip)
    }

    fn fill(&self, tip: &str) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| self.io("creating", e))?;
        let ids = self.store.tree_ids(tip)?;
        let paths: Vec<&String> = ids.keys().collect();
        for chunk in paths.chunks(BATCH) {
            let wanted: Vec<&str> = chunk.iter().map(|p| ids[*p].as_str()).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                self.write(path, &bytes)?;
            }
        }
        Ok(())
    }

    /// What the directory holds that the base does not, as an edit: a put for each file added
    /// or changed and a remove for each file of the base that is gone. Empty where nothing
    /// differs. Refuses a symlink or anything else that is not a regular file, and a name that
    /// is not text.
    pub fn edit(&self) -> Result<Edit, StoreError> {
        Ok(self.delta()?.edit)
    }

    /// Whether the directory differs from the base.
    pub fn is_dirty(&self) -> Result<bool, StoreError> {
        Ok(!self.edit()?.is_empty())
    }

    fn delta(&self) -> Result<Delta, StoreError> {
        let base = self.require_base()?;
        let held = self.store.tree_ids(&base)?;
        // a base holding what the patterns do not would have those files read as removed here
        self.check_held(&held)?;
        let here = self.walk()?;
        let ids = self.local_ids(&here)?;
        let (mut edit, mut sent) = (Edit::new(), BTreeMap::new());
        for path in &here {
            if ids.get(path) != held.get(path) {
                let bytes =
                    std::fs::read(self.dir.join(path)).map_err(|e| self.io("reading", e))?;
                edit = edit.put(path.clone(), bytes);
                sent.insert(path.clone(), ids.get(path).cloned());
            }
        }
        for path in held.keys() {
            if !ids.contains_key(path) {
                edit = edit.remove(path.clone());
                sent.insert(path.clone(), None);
            }
        }
        Ok(Delta {
            base,
            edit,
            sent,
        })
    }

    /// Applies what [`Instance::edit`] finds through the store as an edit built on the base,
    /// then brings the directory to the tip, which holds what the hook made of any file a
    /// writer who won meanwhile changed too. Nothing differing is a no-op that touches neither
    /// the store nor the remote. A hook that refuses leaves the directory and the base as they
    /// were, and the way on is another apply with a hook that merges. When the edit landed and
    /// the directory could not be moved after it (a file was changed during the call), the
    /// error is the move's and the edit stays landed.
    pub fn apply(&self, message: &str, hook: &dyn MergeHook) -> Result<Applied, StoreError> {
        let delta = self.delta()?;
        if delta.edit.is_empty() {
            return Ok(Applied {
                commit:    None,
                changed:   false,
                attempts:  0,
                confirmed: false,
            });
        }
        let tracked = Tracking::new(hook);
        let applied = self
            .store
            .apply_from(Some(&delta.base), &delta.edit, message, &tracked)?;
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
        if self.base()?.is_some() {
            return Err(self.fail("has a record already, so its changes go through apply", Vec::new()));
        }
        if self.store.local_tip()?.is_some() {
            return Err(self.fail("is not made from a ref that already has a tip, which is instantiated", Vec::new()));
        }
        let here = self.walk()?;
        if here.is_empty() {
            return Err(self.fail("holds no files to seed the ref with", Vec::new()));
        }
        let mut edit = Edit::new();
        for path in &here {
            let bytes = std::fs::read(self.dir.join(path)).map_err(|e| self.io("reading", e))?;
            edit = edit.put(path.clone(), bytes);
        }
        // The exclusion goes first: it is the step that can fail on a read-only checkout, and once
        // the ref has a tip a failure after it leaves a directory that neither `seed` (the ref has a
        // tip) nor `apply` (no record) takes.
        self.exclude()?;
        let applied = self.store.apply_from(None, &edit, message, hook)?;
        let Some(tip) = applied.commit.clone() else {
            return Err(self.fail("was seeded and the ref has no commit", Vec::new()));
        };
        if let Err(e) = self.record(&tip) {
            return Err(self.fail(
                "made the ref (tip and cause follow) but could not record the directory as its instance; remove its files and instantiate it",
                vec![tip, e.to_string()],
            ));
        }
        Ok(applied)
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
    fn follow(&self, sent: &BTreeMap<String, Option<String>>) -> Result<(), StoreError> {
        let Some(tip) = self.store.local_tip()? else { return Ok(()) };
        let base = self.require_base()?;
        if base == tip {
            return Ok(());
        }
        let (from, to) = (self.store.tree_ids(&base)?, self.store.tree_ids(&tip)?);
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
        let here = self.local_ids(&moved)?;
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
            let wanted: Vec<&str> = chunk.iter().map(|p| to[*p].as_str()).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                // a directory at a path to be written holds directories only, or `in_the_way` refused
                if self.dir.join(path.as_str()).is_dir() {
                    std::fs::remove_dir_all(self.dir.join(path.as_str()))
                        .map_err(|e| self.io("removing", e))?;
                }
                self.write(path, &bytes)?;
            }
        }
        self.record(&tip)
    }


    fn require_base(&self) -> Result<String, StoreError> {
        if !self.dir.is_dir() {
            return Err(self.fail("is not a directory", Vec::new()));
        }
        self.base()?
            .ok_or_else(|| self.fail("was not made by `materialize`", Vec::new()))
    }


    fn dir_text(&self) -> String {
        self.dir.display().to_string()
    }

    fn fail(&self, why: &'static str, paths: Vec<String>) -> StoreError {
        StoreError::Instance {
            dir: self.dir_text(),
            why,
            paths,
        }
    }

    fn io(&self, what: &str, source: std::io::Error) -> StoreError {
        StoreError::Io {
            context: format!("{what} the instance `{}`", self.dir.display()),
            source,
        }
    }
}

/// `path` with the part that exists resolved through symlinks and `..`. The rest cannot hold a
/// symlink, so it is folded lexically.
fn resolved(path: &Path) -> PathBuf {
    let mut rest: Vec<Component<'_>> = Vec::new();
    let mut head = path;
    loop {
        if let Ok(mut real) = std::fs::canonicalize(head) {
            for part in rest.iter().rev() {
                match part {
                    Component::ParentDir => {
                        real.pop();
                    },
                    Component::CurDir => {},
                    other => real.push(other),
                }
            }
            return real;
        }
        let (Some(parent), Some(last)) = (head.parent(), head.components().next_back()) else {
            return path.to_path_buf();
        };
        rest.push(last);
        head = parent;
    }
}
