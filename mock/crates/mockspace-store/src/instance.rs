//! A directory that holds the files of a ref's tip, for the tools that read files.
//!
//! An instance is a view of a ref, not a checkout of it: nothing is staged, no index file is
//! made, and git's ignore rules do not apply, so every regular file in the directory is data.
//! What the instance remembers is its base, the commit its files were last brought to, and that
//! is kept in the repository's own directory, apart from the files. A change is the difference
//! between the directory and the base, taken as an [`Edit`] (a put for each file added or
//! changed, a remove for each file gone), and it is applied through the store like any other,
//! so a writer whose instance is behind adds its files to the tip that won and removes only
//! what it removed.
//!
//! The directory is excluded from the status of the branch checked out beside it, through the
//! repository's shared exclude file. Changes in an instance exist nowhere else until
//! [`Instance::apply`] runs, and `git clean -fdx` removes an excluded directory with them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::edit::Edit;
use crate::error::StoreError;
use crate::merge::MergeHook;
use crate::ref_history::BATCH;
use crate::ref_store::RefStore;
use crate::store::{Applied, Freshness, Store};

/// The file in the repository's common directory that records the base of every instance.
/// One line each: the base commit, the local ref, the directory, separated by tabs.
const RECORDS: &str = "mock-instances";

/// A directory kept in step with the local ref of a [`RefStore`].
pub struct Instance<'a> {
    store: &'a RefStore,
    dir:   PathBuf,
}

impl<'a> Instance<'a> {
    /// An instance in `dir`, which is made absolute and has any `.` and `..` folded away.
    /// Nothing is read or written yet.
    pub fn new(store: &'a RefStore, dir: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let given = dir.into();
        let absolute = std::path::absolute(&given).map_err(|source| {
            StoreError::Io {
                context: format!("making `{}` absolute", given.display()),
                source,
            }
        })?;
        let mut folded = PathBuf::new();
        for part in absolute.components() {
            match part {
                Component::ParentDir => {
                    folded.pop();
                },
                Component::CurDir => {},
                other => folded.push(other),
            }
        }
        let this = Self {
            store,
            dir: folded,
        };
        match this.dir.to_str() {
            Some(text) if !text.chars().any(char::is_control) => Ok(this),
            _ => Err(this.fail("a path that is not plain text", Vec::new())),
        }
    }

    /// The directory, absolute.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The commit the files of the directory were last brought to, or `None` where no instance
    /// was made here.
    pub fn base(&self) -> Result<Option<String>, StoreError> {
        let (local, dir) = (self.store.pair().local(), self.dir_text());
        Ok(self
            .records()?
            .into_iter()
            .find(|(_, r, d)| r == local && *d == dir)
            .map(|(base, ..)| base))
    }

    /// Writes the files of the local tip into the directory, which must be absent or empty, and
    /// records the tip as the base. Gives the tip. A directory that this call made is removed
    /// again when it fails.
    pub fn materialize(&self) -> Result<String, StoreError> {
        let Some(tip) = self.store.local_tip()? else {
            return Err(StoreError::NotFetched {
                local_ref: self.store.pair().local().to_owned(),
            });
        };
        let existed = self.dir.exists();
        if existed {
            let mut entries = std::fs::read_dir(&self.dir).map_err(|e| self.io("reading", e))?;
            if entries.next().is_some() {
                return Err(self.fail("is not an empty directory", Vec::new()));
            }
        }
        let made = self.fill(&tip);
        if made.is_err() && !existed {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
        made?;
        self.exclude()?;
        self.record(&tip)?;
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
        let base = self.require_base()?;
        let held = self.store.tree_ids(&base)?;
        let here = self.walk()?;
        let ids = self.local_ids(&here)?;
        let mut edit = Edit::new();
        for path in &here {
            if ids.get(path) != held.get(path) {
                let bytes =
                    std::fs::read(self.dir.join(path)).map_err(|e| self.io("reading", e))?;
                edit = edit.put(path.clone(), bytes);
            }
        }
        for path in held.keys() {
            if !ids.contains_key(path) {
                edit = edit.remove(path.clone());
            }
        }
        Ok(edit)
    }

    /// Whether the directory differs from the base.
    pub fn is_dirty(&self) -> Result<bool, StoreError> {
        Ok(!self.edit()?.is_empty())
    }

    /// Applies what [`Instance::edit`] finds through the store, then brings the directory to the
    /// tip, which holds the files of any writer that won meanwhile. Nothing differing is a
    /// no-op that touches neither the store nor the remote. When the edit landed and the
    /// directory could not be moved after it (a file was changed during the call), the error is
    /// the move's and the edit stays landed; [`Instance::update`] reports the same files.
    pub fn apply(&self, message: &str, hook: &dyn MergeHook) -> Result<Applied, StoreError> {
        let edit = self.edit()?;
        if edit.is_empty() {
            return Ok(Applied {
                commit:    None,
                changed:   false,
                attempts:  0,
                confirmed: false,
            });
        }
        let applied = self.store.apply(&edit, message, hook)?;
        self.follow()?;
        Ok(applied)
    }

    /// Fetches the ref and brings the directory to its tip. A file changed here and on the tip
    /// both is a conflict: the call writes nothing and names the files, and the directory keeps
    /// what it had. A file changed here only stays as it is and is still part of the next edit.
    pub fn update(&self) -> Result<Freshness, StoreError> {
        let fresh = self.store.refresh()?;
        self.follow()?;
        Ok(fresh)
    }

    fn follow(&self) -> Result<(), StoreError> {
        let Some(tip) = self.store.local_tip()? else { return Ok(()) };
        let base = self.require_base()?;
        if base == tip {
            return Ok(());
        }
        let (from, to) = (self.store.tree_ids(&base)?, self.store.tree_ids(&tip)?);
        let moved: Vec<String> = from
            .keys()
            .chain(to.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|p| from.get(*p) != to.get(*p))
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
            if local == from.get(path) {
                plan.push(path.clone());
            } else {
                conflicts.push(path.clone());
            }
        }
        if !conflicts.is_empty() {
            return Err(self.fail("holds files changed here and on the tip", conflicts));
        }
        let writes: Vec<&String> = plan.iter().filter(|p| to.contains_key(*p)).collect();
        for chunk in writes.chunks(BATCH) {
            let wanted: Vec<&str> = chunk.iter().map(|p| to[*p].as_str()).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                self.write(path, &bytes)?;
            }
        }
        for path in plan.iter().filter(|p| !to.contains_key(*p)) {
            std::fs::remove_file(self.dir.join(path)).map_err(|e| self.io("removing", e))?;
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

    fn is_file(&self, rel: &str) -> bool {
        self.dir.join(rel).is_file()
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io("creating", e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| self.io("writing", e))
    }

    /// The regular files under the directory, as `/`-separated paths, sorted.
    fn walk(&self) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        self.walk_into(&self.dir, "", &mut out)?;
        Ok(out)
    }

    fn walk_into(&self, dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), StoreError> {
        let mut entries = std::fs::read_dir(dir)
            .map_err(|e| self.io("reading", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.io("reading", e))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(name) = entry.file_name().into_string() else {
                return Err(self.fail("holds a file name that is not text", Vec::new()));
            };
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let kind = entry.file_type().map_err(|e| self.io("reading", e))?;
            if kind.is_symlink() {
                return Err(self.fail("holds a symlink", vec![rel]));
            } else if kind.is_dir() {
                self.walk_into(&entry.path(), &rel, out)?;
            } else if kind.is_file() {
                out.push(rel);
            } else {
                return Err(self.fail("holds something that is not a regular file", vec![rel]));
            }
        }
        Ok(())
    }

    /// The blob id each of `paths` would have if committed as it is, by path. A path that is
    /// not a file here has no entry.
    fn local_ids(&self, paths: &[String]) -> Result<BTreeMap<String, String>, StoreError> {
        let present: Vec<&String> = paths.iter().filter(|p| self.is_file(p)).collect();
        let mut ids = BTreeMap::new();
        for chunk in present.chunks(BATCH) {
            let mut input = String::new();
            for path in chunk {
                input.push_str(&self.dir.join(path).display().to_string());
                input.push('\n');
            }
            let out = self.store.git.must(
                &["hash-object", "--no-filters", "--stdin-paths"],
                &[],
                Some(input.as_bytes()),
            )?;
            let text = out.text();
            let found: Vec<&str> = text.lines().collect();
            if found.len() != chunk.len() {
                return Err(StoreError::Git {
                    command: "hash-object --stdin-paths".to_owned(),
                    detail:  format!("{} ids for {} paths", found.len(), chunk.len()),
                });
            }
            for (path, id) in chunk.iter().zip(found) {
                ids.insert((*path).clone(), id.to_owned());
            }
        }
        Ok(ids)
    }

    fn records(&self) -> Result<Vec<(String, String, String)>, StoreError> {
        let file = self.records_file()?;
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(self.io("reading the records of", e)),
        };
        Ok(text
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                Some((
                    f.next()?.to_owned(),
                    f.next()?.to_owned(),
                    f.next()?.to_owned(),
                ))
            })
            .collect())
    }

    fn record(&self, base: &str) -> Result<(), StoreError> {
        let (local, dir) = (self.store.pair().local().to_owned(), self.dir_text());
        let mut rows = self.records()?;
        rows.retain(|(_, r, d)| !(*r == local && *d == dir));
        rows.push((base.to_owned(), local, dir));
        let mut text = String::new();
        for (b, r, d) in rows {
            text.push_str(&format!("{b}\t{r}\t{d}\n"));
        }
        let file = self.records_file()?;
        let staged = file.with_extension("new");
        std::fs::write(&staged, text).map_err(|e| self.io("writing the records of", e))?;
        std::fs::rename(&staged, &file).map_err(|e| self.io("writing the records of", e))
    }

    fn records_file(&self) -> Result<PathBuf, StoreError> {
        let out = self.store.git.must(
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            &[],
            None,
        )?;
        Ok(PathBuf::from(out.text()).join(RECORDS))
    }

    /// Adds the directory to the exclude file the repository shares between its work trees, when
    /// it lies inside the work tree this store was opened on.
    fn exclude(&self) -> Result<(), StoreError> {
        let top = self
            .store
            .git
            .run(&["rev-parse", "--show-toplevel"], &[], None)?;
        if !top.ok() {
            return Ok(());
        }
        let (Ok(top), Ok(dir)) = (
            std::fs::canonicalize(top.text()),
            std::fs::canonicalize(&self.dir),
        ) else {
            return Ok(());
        };
        let Ok(rel) = dir.strip_prefix(&top) else { return Ok(()) };
        let Some(rel) = rel.to_str().filter(|r| !r.is_empty()) else { return Ok(()) };
        let mut line = String::from("/");
        for c in rel.chars() {
            if "\\*?[]!# ".contains(c) {
                line.push('\\');
            }
            line.push(c);
        }
        line.push('/');
        let out = self.store.git.must(
            &["rev-parse", "--path-format=absolute", "--git-path", "info/exclude"],
            &[],
            None,
        )?;
        let file = PathBuf::from(out.text());
        let held = std::fs::read_to_string(&file).unwrap_or_default();
        if held.lines().any(|l| l == line) {
            return Ok(());
        }
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io("creating the directory of", e))?;
        }
        let sep = if held.is_empty() || held.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&file, format!("{held}{sep}{line}\n"))
            .map_err(|e| self.io("writing the exclude file for", e))
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
