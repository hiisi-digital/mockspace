//! Which files of a guard git wrote over, told from the bytes on disk and from the branch's index.
//!
//! A file of the guard that is gone, or that holds bytes the object database already has, is one
//! git wrote: git writes the bytes of a commit. Two things make that test too narrow, and each is
//! asked here:
//!
//! - git writes a file through the filters the attributes name (`eol=crlf`, `ident`, a `filter`
//!   driver such as LFS), so the bytes on disk are not the bytes of the commit. The file is hashed
//!   again as git would store it, through the same filters, from the top of the work tree so the
//!   attributes of the path apply.
//! - an edit that was guarded, written over and then edited again holds new bytes. Where git wrote
//!   the path, the branch's index holds it outside the sparse checkout (unmerged, or tracked with
//!   no skip-worktree bit), which a file the person edited and git never touched is not. Such a
//!   file is taken for one git wrote over, and the guard's bytes are kept as lost.
//!
//! Every answer is asked before the look writes anything, since an edit it copies is in the
//! database after.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{Delta, Instance};
use crate::error::StoreError;
use crate::git::Git;
use crate::ids::{FileId, Oid};

/// What a look knows of the bytes in the database and of the paths git wrote, asked once.
pub(super) struct Known {
    /// Blobs the database held before the look wrote anything.
    pub(super) blobs:     BTreeSet<Oid>,
    /// Paths git wrote though they hold new bytes: through a filter, or over a guarded edit.
    pub(super) git_wrote: BTreeSet<String>,
}

impl Instance<'_> {
    /// [`Known`] for the files of `guard` that differ from it now.
    pub(super) fn known(&self, delta: &Delta, guard: &[(&String, &FileId)]) -> Result<Known, StoreError> {
        // what a base holds is in the database; what differs from it is asked
        let mut blobs: BTreeSet<Oid> = delta.base_ids.values().map(|id| id.blob.clone()).collect();
        blobs.extend(self.in_object_database(delta.sent.values().flatten().map(|id| &id.blob))?);
        let unsure: Vec<(&String, &FileId)> = guard
            .iter()
            .filter(|(path, _)| delta.here.get(*path).is_some_and(|now| !blobs.contains(&now.blob)))
            .copied()
            .collect();
        let mut git_wrote = BTreeSet::new();
        if unsure.is_empty() {
            return Ok(Known { blobs, git_wrote });
        }
        let Some((top, rel)) = self.from_top()? else { return Ok(Known { blobs, git_wrote }) };
        let in_tree = |path: &str| if rel.is_empty() { path.to_owned() } else { format!("{rel}/{path}") };
        let filtered = self.filtered_ids(&top, unsure.iter().map(|(p, _)| in_tree(p)).collect());
        let stored = self.in_object_database(filtered.values())?;
        for (path, _) in &unsure {
            if filtered.get(&in_tree(path)).is_some_and(|id| stored.contains(id) || blobs.contains(id)) {
                git_wrote.insert((*path).clone());
            }
        }
        // a guarded edit edited again since: git wrote it only where the index holds the path
        let edits: Vec<String> = unsure
            .iter()
            .filter(|(path, id)| !git_wrote.contains(*path) && delta.base_ids.get(*path) != Some(*id))
            .map(|(path, _)| in_tree(path))
            .collect();
        for path in self.written_by_the_index(&top, &edits)? {
            let path = if rel.is_empty() { path } else { path.strip_prefix(&format!("{rel}/")).map(str::to_owned).unwrap_or(path) };
            git_wrote.insert(path);
        }
        Ok(Known { blobs, git_wrote })
    }

    /// Of `blobs`, the ones the object database holds.
    pub(super) fn in_object_database<'b>(&self, blobs: impl Iterator<Item = &'b Oid>) -> Result<BTreeSet<Oid>, StoreError> {
        let blobs: Vec<&Oid> = blobs.collect();
        if blobs.is_empty() {
            return Ok(BTreeSet::new());
        }
        let input: String = blobs.iter().map(|b| format!("{b}\n")).collect();
        let out = self.store.git.must(&["cat-file", "--batch-check=%(objectname)"], &[], Some(input.as_bytes()))?;
        Ok(out.text().lines().filter_map(|l| Oid::parse(l).ok()).collect())
    }

    /// The top of the work tree the instance lies in, and the instance's path from it (empty where
    /// it is the top). `None` where it lies in no work tree, and no attribute or index reaches it.
    fn from_top(&self) -> Result<Option<(PathBuf, String)>, StoreError> {
        let top = self.store.git.run(&["rev-parse", "--show-toplevel"], &[], None)?;
        if !top.ok() {
            return Ok(None);
        }
        let (Ok(top), Ok(dir)) = (std::fs::canonicalize(top.text()), std::fs::canonicalize(&self.dir)) else {
            return Ok(None);
        };
        let Ok(rel) = dir.strip_prefix(&top) else { return Ok(None) };
        let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        Ok(Some((top, rel)))
    }

    /// The id each of `paths` (from the top of the work tree) has hashed as git stores it, through
    /// the filters its attributes name. A path a filter fails on is left out: its raw id is the
    /// only one known, as before.
    fn filtered_ids(&self, top: &std::path::Path, paths: Vec<String>) -> BTreeMap<String, Oid> {
        let git = Git::new(top.to_path_buf());
        let mut out = BTreeMap::new();
        for path in paths {
            // one path a call, so a filter that fails on one leaves the others answered
            let hashed = git.run(&["hash-object", "--stdin-paths"], &[], Some(format!("{path}\n").as_bytes()));
            if let Some(id) = hashed.ok().filter(|o| o.ok()).and_then(|o| Oid::parse(&o.text()).ok()) {
                out.insert(path, id);
            }
        }
        out
    }

    /// Of `paths` (from the top of the work tree), the ones the branch's index holds as only git
    /// puts them there: unmerged, or tracked without the skip-worktree bit the sparse checkout
    /// gives every path of an instance.
    fn written_by_the_index(&self, top: &std::path::Path, paths: &[String]) -> Result<Vec<String>, StoreError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["--literal-pathspecs", "ls-files", "-z", "-t", "--"];
        args.extend(paths.iter().map(String::as_str));
        let out = Git::new(top.to_path_buf()).must(&args, &[], None)?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        Ok(text
            .split('\0')
            .filter_map(|entry| entry.split_once(' '))
            .filter(|(tag, _)| !tag.eq_ignore_ascii_case("S"))
            .map(|(_, path)| path.to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
}
