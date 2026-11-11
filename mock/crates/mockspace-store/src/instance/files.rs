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
        // every look the engine takes at the instance keeps a copy of what is edited in it
        self.snapshot_of(&delta)?;
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

    /// Whether the executable bit of a file here means anything: git's `core.fileMode`, true where
    /// it is not set. Where it is off the filesystem reports a mode that is not the file's own (a
    /// mount that makes every file executable), and the bit is taken from the base instead.
    pub(super) fn file_mode(&self) -> Result<bool, StoreError> {
        if let Some(on) = self.file_mode.get() {
            return Ok(*on);
        }
        let out = self.store.git.run(&["config", "--type=bool", "--get", "core.fileMode"], &[], None)?;
        let on = match out.code {
            Some(0) => out.text() != "false",
            Some(1) => true,
            _ => {
                return Err(StoreError::Git {
                    command: "config --get core.fileMode".to_owned(),
                    detail:  out.stderr,
                });
            },
        };
        let _ = self.file_mode.set(on);
        Ok(on)
    }

    /// The regular files under the directory, as `/`-separated paths, sorted. Every regular file
    /// is data, whatever a `.gitignore` says: a ref holds the files its patterns match. A cache
    /// directory (one with a `CACHEDIR.TAG` that carries the standard signature, which is what
    /// cargo writes into a `target`) is a build's output and is not read, unless the base, or the
    /// local tip where there is no record, holds a file under it.
    pub(super) fn walk(&self) -> Result<Vec<String>, StoreError> {
        let known: BTreeSet<String> = match self.base()?.or(self.store.local_tip()?) {
            Some(commit) => self.store.tree_ids(&commit)?.into_keys().collect(),
            None => BTreeSet::new(),
        };
        let mut out = Vec::new();
        self.walk_into(&self.dir, "", &known, &mut out)?;
        Ok(out)
    }

    /// Whether the directory at `at` is a cache directory, by the signature the cache directory
    /// tag specification names.
    fn is_cache_dir(at: &Path) -> bool {
        use std::io::Read;
        const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
        let mut head = [0u8; SIGNATURE.len()];
        std::fs::File::open(at.join("CACHEDIR.TAG")).and_then(|mut f| f.read_exact(&mut head)).is_ok() && head == *SIGNATURE
    }

    fn walk_into(
        &self,
        dir: &Path,
        prefix: &str,
        known: &BTreeSet<String>,
        out: &mut Vec<String>,
    ) -> Result<(), StoreError> {
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
                let below = format!("{rel}/");
                let holds_some = known.range(below.clone()..).next().is_some_and(|k| k.starts_with(&below));
                if self.held.as_ref().is_some_and(|h| !h.may_hold_under(&rel)) {
                    // nothing of the patterns can be in there
                } else if !holds_some && Self::is_cache_dir(&entry.path()) {
                    // a build's output: not data, and not named a loss because it is not read
                } else {
                    self.walk_into(&entry.path(), &rel, known, out)?;
                }
            } else if !self.holds(&rel) {
                // the branch's own: not this instance's to read or to refuse
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

    /// The blob id each of `paths` would have if committed as it is, by path. A path that is not
    /// a file here has no entry. The executable bit is the file's own where `core.fileMode` is
    /// on, and else the one `modes` holds for the path (ids of the base or the tip), none for a
    /// path it does not hold. A file whose size, times and inode are what they were when it was
    /// hashed, and that was already old enough then for a later write to show, is not hashed
    /// again; `whole` says `paths` is every file of the directory, so the cache forgets the rest.
    pub(super) fn local_ids<S: AsRef<str>>(
        &self,
        paths: &[S],
        modes: &BTreeMap<String, String>,
        whole: bool,
    ) -> Result<BTreeMap<String, String>, StoreError> {
        use std::os::unix::fs::PermissionsExt;
        let trust_mode = self.file_mode()?;
        let mut stats = self.load_stats();
        let mut ids = BTreeMap::new();
        let mut hashed: Vec<(&str, super::stats::Stamp, bool)> = Vec::new();
        for path in paths.iter().map(AsRef::as_ref) {
            let Ok(meta) = std::fs::metadata(self.dir.join(path)) else { continue };
            if !meta.is_file() {
                continue;
            }
            let exec = if trust_mode {
                meta.permissions().mode() & 0o100 != 0
            } else {
                modes.get(path).is_some_and(|id| crate::ref_history::split_id(id).1)
            };
            let stamp = super::stats::Stamp::of(&meta);
            match stats.trusted(path, &stamp) {
                Some(id) => {
                    ids.insert(path.to_owned(), with_mode(id, exec));
                },
                None => hashed.push((path, stamp, exec)),
            }
        }
        let at = super::stats::now();
        for chunk in hashed.chunks(BATCH) {
            let mut input = String::new();
            for (path, ..) in chunk {
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
            for ((path, stamp, exec), id) in chunk.iter().zip(found) {
                stats.put(path, *stamp, id, at);
                ids.insert((*path).to_owned(), with_mode(id, *exec));
            }
        }
        if whole {
            stats.retain(|p| ids.contains_key(p));
        }
        self.save_stats(&stats);
        Ok(ids)
    }
}

/// `id` with the executable marker the ids of a tree carry, where `exec`.
fn with_mode(id: &str, exec: bool) -> String {
    if exec { format!("{id}{}", crate::ref_history::EXEC) } else { id.to_owned() }
}
