//! A directory that holds the files of a ref's tip, for the tools that read files.
//!
//! An instance is a view of a ref, not a checkout of it: nothing is staged and no index file is
//! made. Every regular file in the directory is data, less what a `.gitignore` keeps out of it
//! and what a cache directory holds (see `walk`).
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
//! [`Instance::apply`] runs, but for the copy `backup` keeps, and `git clean -fdx` removes an
//! excluded directory with them.
//! Nothing here guards a file saved by another process in the instant between the comparison
//! and the write of [`Instance::update`]; an instance is not edited while that runs.

mod apply;
mod backup;
mod overwritten;
mod pending;
mod files;
mod held;
mod lines;
mod records;
mod stats;
mod tracking;
mod walk;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub use backup::Look;
pub use held::Held;
pub use lines::{exclude_lines, sparse_lines};

use crate::edit::Edit;
use crate::error::StoreError;
use crate::ids::{Base, FileId, Oid};
use crate::ref_history::BATCH;
use crate::ref_store::RefStore;

/// What a fill wrote: the files, from the instance, and the directories it made, in the order
/// it made them.
#[derive(Default)]
struct Wrote {
    files: Vec<String>,
    dirs:  Vec<PathBuf>,
}

/// A directory kept in step with the local ref of a [`RefStore`].
pub struct Instance<'a> {
    store: &'a RefStore,
    dir:   PathBuf,
    held:  Option<Held>,
    // asked of git once, since each is a process or two and several steps want it
    record_at: std::sync::OnceLock<PathBuf>,
    file_mode: std::sync::OnceLock<bool>,
}

/// What the directory holds against the base: the base and the ids of its files, the id of every
/// file of the instance on disk, and of those that differ the id each has now (`None` for a file
/// of the base that is gone), which says afterwards whether a file still is what was sent.
struct Delta {
    base:     Base,
    base_ids: BTreeMap<String, FileId>,
    here:     BTreeMap<String, FileId>,
    sent:     BTreeMap<String, Option<FileId>>,
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
            record_at: std::sync::OnceLock::new(),
            file_mode: std::sync::OnceLock::new(),
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

    /// Refuses a directory that already holds a file the instance would hold, naming each. What a
    /// `.gitignore` keeps out and what a cache directory holds is no file of the instance, is never
    /// read, removed or written by it, and stands in nobody's way: a clone that built in the directory
    /// before the ref reached it keeps its build output and its ignored files.
    fn holds_nothing_yet(&self) -> Result<(), StoreError> {
        let here = self.walk()?;
        if here.is_empty() {
            return Ok(());
        }
        let why = if self.held.is_some() { "already holds files its patterns match" } else { "already holds files" };
        Err(self.fail(why, here))
    }

    /// Refuses a tree of the ref that holds a path this instance does not: written here it
    /// would be a file the branch sees and no later edit of the instance could name.
    fn check_held(&self, ids: &BTreeMap<String, FileId>) -> Result<(), StoreError> {
        let outside: Vec<String> = ids.keys().filter(|p| !self.holds(p)).cloned().collect();
        if outside.is_empty() {
            Ok(())
        } else {
            Err(self.fail("is of a ref that holds files outside its patterns", outside))
        }
    }

    /// The commit the files of the directory were last brought to, or `None` where no instance
    /// was made here.
    /// For an instance of a ref not born yet it is [`Base::Unborn`], which holds no file.
    pub fn base(&self) -> Result<Option<Base>, StoreError> {
        self.read_record()
    }

    /// Whether the directory is an instance of a ref that had no commit when it was made, and has
    /// had none brought to it since: its base is no commit, and its first apply makes the ref.
    pub fn is_unborn(&self) -> Result<bool, StoreError> {
        Ok(self.base()?.is_some_and(|b| b.is_unborn()))
    }

    /// Makes the directory an instance of a ref the remote does not have yet, which the caller
    /// has asked the remote for: the directory is made if absent and must hold no file its
    /// patterns match, and the record has no commit as its base. Refused where the ref has a tip
    /// (it is instantiated with [`Instance::materialize`]) and where a record exists.
    pub fn begin(&self) -> Result<(), StoreError> {
        if self.store.local_tip()?.is_some() {
            return Err(self.fail("is of a ref that has a tip, so it is instantiated from it", Vec::new()));
        }
        if self.base()?.is_some() {
            return Err(self.fail("has a record already", Vec::new()));
        }
        let existed = self.dir.exists();
        if existed {
            self.holds_nothing_yet()?;
        }
        let made = std::fs::create_dir_all(&self.dir)
            .map_err(|e| self.io("creating", e))
            .and_then(|()| self.exclude())
            .and_then(|()| self.record(&Base::Unborn));
        if made.is_err() {
            self.forget();
            if !existed {
                let _ = std::fs::remove_dir(&self.dir);
            }
        }
        made
    }

    /// Writes the files of the local tip into the directory, which must hold no file of the instance, and
    /// records the tip as the base. Gives the tip. An empty directory that has a record, whose
    /// base holds files, is one where every file was removed, and is refused: the removals are
    /// an edit not yet applied, and the directory itself removed starts the instance again.
    /// Nothing may stand where the tip writes, a file git ignores included. When it fails after
    /// writing, the files it wrote, the directories it made and the record are removed again, and
    /// nothing else.
    pub fn materialize(&self) -> Result<String, StoreError> {
        let Some(tip) = self.store.local_oid()? else {
            return Err(StoreError::NotFetched {
                local_ref: self.store.pair().local().to_owned(),
            });
        };
        let existed = self.dir.exists();
        if existed {
            self.holds_nothing_yet()?;
            if let Some(base) = self.base()? {
                if !self.ids_of(&base)?.is_empty() {
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
        self.check_room(&ids)?;
        let mut wrote = Wrote::default();
        let made = self
            .fill(&ids, &mut wrote)
            .and_then(|()| self.exclude())
            .and_then(|()| self.record(&Base::At(tip.clone())));
        if made.is_err() {
            self.forget();
            // only what the fill wrote goes: the directory may hold the branch's files, or files
            // git ignores, and those are the person's
            for path in &wrote.files {
                let _ = std::fs::remove_file(self.dir.join(path));
            }
            for dir in wrote.dirs.iter().rev() {
                let _ = std::fs::remove_dir(dir);
            }
            if !existed {
                let _ = std::fs::remove_dir(&self.dir);
            }
        }
        made?;
        Ok(tip.to_string())
    }

    /// Refuses a tip that cannot be written without writing over or under something here: a
    /// path of the tip where anything stands already, or one below something that is not a
    /// directory. A file git ignores is the person's, so it is named, never written over.
    fn check_room(&self, ids: &BTreeMap<String, FileId>) -> Result<(), StoreError> {
        let writes: Vec<&String> = ids.keys().collect();
        let mut blocked = self.in_the_way(&[], &writes)?;
        blocked.extend(writes.iter().filter(|p| std::fs::symlink_metadata(self.dir.join(p.as_str())).is_ok()).map(|p| (*p).clone()));
        blocked.sort();
        blocked.dedup();
        if blocked.is_empty() { Ok(()) } else { Err(self.fail("holds files in the way of the tip", blocked)) }
    }

    /// Writes the files of `ids` into the directory, noting in `wrote` each file it wrote and
    /// each directory it made, so a failure later takes back those and nothing else.
    fn fill(&self, ids: &BTreeMap<String, FileId>, wrote: &mut Wrote) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| self.io("creating", e))?;
        let paths: Vec<&String> = ids.keys().collect();
        for chunk in paths.chunks(BATCH) {
            let wanted: Vec<&Oid> = chunk.iter().map(|p| &ids[*p].blob).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                let at = self.dir.join(path.as_str());
                let mut missing: Vec<PathBuf> =
                    at.ancestors().skip(1).take_while(|d| *d != self.dir && !d.exists()).map(Path::to_path_buf).collect();
                missing.reverse();
                // noted before the write, so a write that fails half way is taken back too: nothing
                // stood at these paths, `check_room` saw to that
                wrote.dirs.extend(missing);
                wrote.files.push((*path).clone());
                self.write(path, &bytes, ids[*path].mode.is_exec())?;
            }
        }
        Ok(())
    }

    /// What the directory holds that the base does not, as an edit: a put for each file added
    /// or changed and a remove for each file of the base that is gone. Empty where nothing
    /// differs. Refuses a symlink or anything else that is not a regular file, and a name that
    /// is not text.
    pub fn edit(&self) -> Result<Edit, StoreError> {
        self.edit_of(&self.delta()?)
    }

    /// Whether the directory differs from the base.
    pub fn is_dirty(&self) -> Result<bool, StoreError> {
        Ok(!self.delta()?.sent.is_empty())
    }

    /// The ids of the files `base` holds: none for the base of a ref not born yet.
    pub(super) fn ids_of(&self, base: &Base) -> Result<BTreeMap<String, FileId>, StoreError> {
        match base.commit() {
            Some(commit) => self.store.tree_ids(commit),
            None => Ok(BTreeMap::new()),
        }
    }

    fn delta(&self) -> Result<Delta, StoreError> {
        let base = self.require_base()?;
        let base_ids = self.ids_of(&base)?;
        // a base holding what the patterns do not would have those files read as removed here
        self.check_held(&base_ids)?;
        let files = self.walk()?;
        let here = self.local_ids(&files, &base_ids, true)?;
        let mut sent = BTreeMap::new();
        for (path, id) in &here {
            if base_ids.get(path) != Some(id) {
                sent.insert(path.clone(), Some(id.clone()));
            }
        }
        for path in base_ids.keys() {
            if !here.contains_key(path) {
                sent.insert(path.clone(), None);
            }
        }
        Ok(Delta {
            base,
            base_ids,
            here,
            sent,
        })
    }

    /// The edit `delta` names, with the bytes of each file it puts read off the disk now.
    fn edit_of(&self, delta: &Delta) -> Result<Edit, StoreError> {
        let mut edit = Edit::new();
        for (path, id) in &delta.sent {
            edit = match id {
                None => edit.remove(path.clone()),
                Some(id) => {
                    let bytes = std::fs::read(self.dir.join(path)).map_err(|e| self.io("reading", e))?;
                    if id.mode.is_exec() { edit.put_exec(path.clone(), bytes) } else { edit.put(path.clone(), bytes) }
                },
            };
        }
        Ok(edit)
    }

    fn require_base(&self) -> Result<Base, StoreError> {
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
