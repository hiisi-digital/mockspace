//! What an instance reads off its directory and writes into it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::Instance;
use crate::error::StoreError;
use crate::ref_history::BATCH;

impl Instance<'_> {
    /// Of `writes`, the paths that cannot be written once `removals` are gone: one under
    /// something that is not a directory and stays, or one that is a directory still holding
    /// anything but directories. It is asked of the disk and not of the files the instance
    /// holds, since a file of the branch stands in the way as well as one of the instance does.
    pub(super) fn in_the_way(
        &self,
        removals: &[&String],
        writes: &[&String],
    ) -> Result<Vec<String>, StoreError> {
        let gone: BTreeSet<&str> = removals.iter().map(|p| p.as_str()).collect();
        let mut blocked = Vec::new();
        for path in writes {
            let under_a_file = path.match_indices('/').any(|(i, _)| {
                let above = &path[.. i];
                !gone.contains(above)
                    && std::fs::symlink_metadata(self.dir.join(above)).is_ok_and(|m| !m.is_dir())
            });
            let at = self.dir.join(path.as_str());
            let holds_files = std::fs::symlink_metadata(&at).is_ok_and(|m| m.is_dir())
                && self.stays_under(&at, path, &gone)?;
            if under_a_file || holds_files {
                blocked.push((*path).clone());
            }
        }
        Ok(blocked)
    }

    /// Whether the directory `dir`, which is `rel` from the instance, holds anything that is
    /// neither a directory nor one of `gone`.
    fn stays_under(&self, dir: &Path, rel: &str, gone: &BTreeSet<&str>) -> Result<bool, StoreError> {
        for entry in std::fs::read_dir(dir).map_err(|e| self.io("reading", e))? {
            let entry = entry.map_err(|e| self.io("reading", e))?;
            let Ok(name) = entry.file_name().into_string() else { return Ok(true) };
            let below = format!("{rel}/{name}");
            let kind = entry.file_type().map_err(|e| self.io("reading", e))?;
            let stays = if kind.is_dir() {
                self.stays_under(&entry.path(), &below, gone)?
            } else {
                !gone.contains(below.as_str())
            };
            if stays {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Refuses where one of `paths`, or a directory above it, is a symlink: a write or a removal
    /// would go through it to wherever it points, outside the instance.
    pub(super) fn check_parents<S: AsRef<str>>(&self, paths: &[S]) -> Result<(), StoreError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut links = Vec::new();
        for path in paths {
            let path = path.as_ref();
            for i in path.match_indices('/').map(|(i, _)| i).chain([path.len()]) {
                let above = &path[.. i];
                if seen.insert(above)
                    && std::fs::symlink_metadata(self.dir.join(above)).is_ok_and(|m| m.is_symlink())
                {
                    links.push(above.to_owned());
                }
            }
        }
        if links.is_empty() {
            Ok(())
        } else {
            Err(self.fail("has a symlink where the ref has a file or a directory", links))
        }
    }

    /// Writes again every file of the base that is missing here, and gives how many. It is the
    /// way back from a directory whose files were removed by something that did not mean an
    /// edit (a clean of the work tree): the removals are dropped, and a file that is here is
    /// left as it is.
    pub fn restore(&self) -> Result<usize, StoreError> {
        let Some(base) = self.base()? else {
            return Err(self.fail("was not made by `materialize`", Vec::new()));
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| self.io("creating", e))?;
        let ids = self.store.tree_ids(&base)?;
        self.check_held(&ids)?;
        let missing: Vec<&String> = ids
            .keys()
            .filter(|p| std::fs::symlink_metadata(self.dir.join(p.as_str())).is_err())
            .collect();
        self.check_parents(&missing)?;
        let in_the_way = self.in_the_way(&[], &missing)?;
        if !in_the_way.is_empty() {
            return Err(self.fail("holds files in the way of the base", in_the_way));
        }
        for chunk in missing.chunks(BATCH) {
            let wanted: Vec<&str> = chunk.iter().map(|p| ids[*p].as_str()).collect();
            for (path, bytes) in chunk.iter().zip(self.store.blobs(&wanted)?) {
                self.write(path, &bytes, crate::ref_history::split_id(&ids[*path]).1)?;
            }
        }
        self.exclude()?;
        Ok(missing.len())
    }

    /// The files the instance holds, on disk or at the base, and of those the ones that differ
    /// from the base: added, changed or removed here. Both are what the directory holds where no
    /// instance was made, with nothing changed.
    pub fn files(&self) -> Result<(BTreeSet<String>, BTreeSet<String>), StoreError> {
        if self.base()?.is_none() || !self.dir.is_dir() {
            let here = if self.dir.is_dir() { self.walk()? } else { Vec::new() };
            return Ok((here.into_iter().collect(), BTreeSet::new()));
        }
        let delta = self.delta()?;
        let mut held: BTreeSet<String> = self.store.tree_ids(&delta.base)?.into_keys().collect();
        held.extend(self.walk()?);
        Ok((held, delta.sent.into_keys().collect()))
    }

    /// How many files of the base are on disk, and how many the base holds. Both are zero where
    /// no instance was made.
    pub fn present(&self) -> Result<(usize, usize), StoreError> {
        let Some(base) = self.base()? else { return Ok((0, 0)) };
        let ids = self.store.tree_ids(&base)?;
        let here = ids.keys().filter(|p| self.dir.join(p.as_str()).is_file()).count();
        Ok((here, ids.len()))
    }

    /// Removes the directories above `rel` that its removal left empty, never the instance's own.
    pub(super) fn prune(&self, rel: &str) {
        let mut at = self.dir.join(rel);
        while at.pop() && at != self.dir {
            if std::fs::remove_dir(&at).is_err() {
                break;
            }
        }
    }

    pub(super) fn is_file(&self, rel: &str) -> bool {
        self.dir.join(rel).is_file()
    }

    /// Write `bytes` at `rel` with the executable bit as given: set on the owner, group and
    /// other classes that can read, cleared on all of them otherwise.
    pub(super) fn write(&self, rel: &str, bytes: &[u8], exec: bool) -> Result<(), StoreError> {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io("creating", e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| self.io("writing", e))?;
        self.set_exec(&path, exec)
    }

    /// Give the file at `path` the executable bit or take it away, leaving the other bits.
    fn set_exec(&self, path: &Path, exec: bool) -> Result<(), StoreError> {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| self.io("reading", e))?;
        let mode = meta.permissions().mode();
        let want = if exec { mode | ((mode & 0o444) >> 2) } else { mode & !0o111 };
        if want != mode {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(want))
                .map_err(|e| self.io("setting the mode of", e))?;
        }
        Ok(())
    }

    /// Whether the file at `rel` has the executable bit for its owner, as git reads it.
    pub(super) fn is_exec(&self, rel: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(self.dir.join(rel)).is_ok_and(|m| m.permissions().mode() & 0o100 != 0)
    }

    /// The regular files under the directory, as `/`-separated paths, sorted, without the ones a
    /// `.gitignore` ignores (a build's output) unless the base, or the local tip where there is
    /// no record, holds them. Those are the branch's ignored files, not the instance's.
    pub(super) fn walk(&self) -> Result<Vec<String>, StoreError> {
        let known = match self.base()?.or(self.store.local_tip()?) {
            Some(commit) => self.store.tree_ids(&commit)?.into_keys().collect(),
            None => BTreeSet::new(),
        };
        self.walk_keeping(&known)
    }

    /// [`Self::walk`] with `keep`, paths that stay in even where a `.gitignore` ignores them.
    pub(super) fn walk_keeping(&self, keep: &BTreeSet<String>) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        self.walk_into(&self.dir, "", &mut out)?;
        let ignored = self.ignored()?;
        if !ignored.is_empty() {
            out.retain(|p| keep.contains(p) || !ignored.iter().any(|i| p == i || (i.ends_with('/') && p.starts_with(i.as_str()))));
        }
        Ok(out)
    }

    /// What the directory holds that a `.gitignore` of the branch ignores, as paths relative to
    /// the directory, a wholly ignored directory as its path with a closing `/`. Only the
    /// `.gitignore` files are asked: the clone's own exclusions list this instance's files.
    fn ignored(&self) -> Result<Vec<String>, StoreError> {
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        let dir = self.dir.display().to_string();
        let out = self.store.git.run(
            &["-C", &dir, "ls-files", "-z", "--others", "--ignored", "--directory", "--exclude-per-directory=.gitignore"],
            &[],
            None,
        )?;
        if !out.ok() {
            return Ok(Vec::new());
        }
        Ok(out.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()).map(|e| String::from_utf8_lossy(e).into_owned()).collect())
    }

    pub(super) fn walk_into(&self, dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), StoreError> {
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
            if kind.is_dir() {
                if self.held.as_ref().is_none_or(|h| h.may_hold_under(&rel)) {
                    self.walk_into(&entry.path(), &rel, out)?;
                }
            } else if !self.holds(&rel) {
                // the branch's own, or a build's: not this instance's to read or to refuse
            } else if kind.is_symlink() {
                return Err(self.fail("holds a symlink", vec![rel]));
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
    pub(super) fn local_ids<S: AsRef<str>>(
        &self,
        paths: &[S],
    ) -> Result<BTreeMap<String, String>, StoreError> {
        let present: Vec<&str> = paths
            .iter()
            .map(AsRef::as_ref)
            .filter(|p| self.is_file(p))
            .collect();
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
                let id = if self.is_exec(path) { format!("{id}{}", crate::ref_history::EXEC) } else { id.to_owned() };
                ids.insert((*path).to_owned(), id);
            }
        }
        Ok(ids)
    }
}
