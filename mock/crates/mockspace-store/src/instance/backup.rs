//! A copy of the files of an instance, kept in a ref of the repository's own, and what it says of
//! a file git wrote over.
//!
//! An instance's files are hidden from the branch, and git treats a hidden file as expendable: a
//! merge or a rebase of a commit from before the migration writes its bytes over an edit and keeps
//! no copy. So each instance keeps three sets of files in `refs/mock-backup/<namespace>`, under a
//! directory named by its record (one ref for every instance of the namespace in the repository,
//! each worktree's under its own name):
//!
//! - `edit`, the files added or changed against the base, copied whenever the engine looks at the
//!   instance;
//! - `guard`, every file of the instance as it was when git was about to move `ORIG_HEAD` (a hook
//!   runs `data-refs snapshot`, see the engine), which a merge, a rebase and a reset do ahead of
//!   writing the work tree;
//! - `lost`, the files git wrote over: the next look finds a file of the guard that git wrote (see
//!   `overwritten`), and keeps the guard's bytes here. A file stays lost until it holds those bytes
//!   again ([`Instance::restore_written_over`]) or the bytes on disk are taken
//!   ([`Instance::accept_written_over`]).
//!
//! Each state is one commit whose parent is the last, so every earlier copy is in the history of
//! the ref. The ref is local and never pushed. Its files are read with
//! `git show refs/mock-backup/<namespace>:<record>/<set>/<path>`.
//!
//! The copy is written with a compare-and-swap of the ref, re-read and built again on a lost race,
//! so two worktrees copying at once both keep theirs. A guard the ref cannot take is kept beside
//! the record and folded in by the next look (see `pending`). Any other failure to write it is said
//! by the caller and fails no command that only reads; a failure to read it is an error, since what
//! was written over is then not known.
//!
//! What this does not cover is a write git makes with no reference moving before it and no engine
//! command since the edit: `git cherry-pick` of a commit from before the migration is one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::overwritten::Known;
use super::{Delta, Instance};
use crate::error::StoreError;
use crate::ids::{FileId, Oid};

/// How many times a copy is built again on a lost compare-and-swap before the failure is given.
const ATTEMPTS: u64 = 8;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// What a look at an instance found of its copy.
#[derive(Debug)]
pub struct Look {
    /// The files git wrote over since they were last as the copy keeps them, by path from the
    /// instance, sorted.
    pub written_over: Vec<String>,
    /// Whether the copy was written. A failure is the caller's to say, and fails no command that
    /// only reads.
    pub copied:       Result<(), StoreError>,
}

/// The three sets of an instance's copy, by path from the instance.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
struct Copy {
    edit:  BTreeMap<String, FileId>,
    guard: BTreeMap<String, FileId>,
    lost:  BTreeMap<String, FileId>,
}

impl Copy {
    fn sets(&self) -> [(&'static str, &BTreeMap<String, FileId>); 3] {
        [("edit", &self.edit), ("guard", &self.guard), ("lost", &self.lost)]
    }

    fn blobs(&self) -> BTreeSet<&Oid> {
        self.sets().into_iter().flat_map(|(_, s)| s.values().map(|id| &id.blob)).collect()
    }

    fn is_empty(&self) -> bool {
        self.sets().iter().all(|(_, s)| s.is_empty())
    }
}

/// What a look does to the copy besides copying the edit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Keep {
    /// Nothing more.
    Look,
    /// Sets the guard to every file now on disk, before git writes.
    Guard,
    /// Takes the bytes on disk for every file written over.
    Accept,
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Instance<'_> {
    /// The ref the copies of this instance's files are kept in.
    pub fn backup_ref(&self) -> String {
        let local = self.store.pair().local();
        let name = local.strip_prefix("refs/mock/").unwrap_or_else(|| local.trim_start_matches("refs/"));
        format!("refs/mock-backup/{name}")
    }

    /// Copies the files edited here, and says which files git wrote over since the last look. A
    /// directory that is no instance has nothing to say; one git removed whole has every file of the
    /// copy made before git wrote to say.
    pub fn look(&self) -> Result<Look, StoreError> {
        self.keep(Keep::Look).map(|(look, _)| look)
    }

    /// [`Instance::look`], then keeps every file of the instance as it is now, so that the next
    /// look can tell a file git wrote over. Run before git writes the work tree.
    pub fn guard(&self) -> Result<Look, StoreError> {
        self.keep(Keep::Guard).map(|(look, _)| look)
    }

    /// Takes the bytes on disk for every file git wrote over: the copy stops naming them, and keeps
    /// what they held in its history.
    pub fn accept_written_over(&self) -> Result<Look, StoreError> {
        self.keep(Keep::Accept).map(|(look, _)| look)
    }

    /// Writes back the bytes each of `paths` held before git wrote over it, from the copy. Refused,
    /// with nothing written, where one of them is not a file git wrote over, or where a symlink or
    /// a file stands where the write would go. What the file holds now is copied first, as an
    /// edit, so it is in the history of the copy.
    pub fn restore_written_over(&self, paths: &[String]) -> Result<(), StoreError> {
        let (look, copy) = self.keep(Keep::Look)?;
        // what the files hold now is in no copy, so nothing is written over it
        look.copied.map_err(|e| StoreError::Io {
            context: format!(
                "copying what the files of the instance `{}` hold now, before writing the bytes from before over them",
                self.dir.display()
            ),
            source:  std::io::Error::other(e.to_string()),
        })?;
        let unknown: Vec<String> = paths.iter().filter(|p| !copy.lost.contains_key(*p)).cloned().collect();
        if !unknown.is_empty() {
            return Err(self.fail("names files git did not write over", unknown));
        }
        self.check_parents(paths)?;
        let writes: Vec<&String> = paths.iter().collect();
        let in_the_way = self.in_the_way(&[], &writes)?;
        if !in_the_way.is_empty() {
            return Err(self.fail("holds files in the way of the copy", in_the_way));
        }
        let wanted: Vec<&Oid> = paths.iter().map(|p| &copy.lost[p].blob).collect();
        for (path, bytes) in paths.iter().zip(self.store.blobs(&wanted)?) {
            if self.dir.join(path).is_dir() {
                std::fs::remove_dir_all(self.dir.join(path)).map_err(|e| self.io("removing", e))?;
            }
            self.write(path, &bytes, copy.lost[path].mode.is_exec())?;
        }
        // the file holds the bytes again, so the copy stops naming it; a copy that cannot be written
        // now names it no more on the next look, which finds the same
        let _ = self.keep(Keep::Look);
        Ok(())
    }

    fn keep(&self, keep: Keep) -> Result<(Look, Copy), StoreError> {
        let Some(base) = self.base()? else {
            let look = Look {
                written_over: Vec::new(),
                copied:       Ok(()),
            };
            return Ok((look, Copy::default()));
        };
        // a directory git removed whole is an instance with none of its files, and what the copy kept
        // of them before is what git wrote over
        let delta = if self.dir.is_dir() {
            self.delta()?
        } else {
            let base_ids = self.ids_of(&base)?;
            let sent = base_ids.keys().map(|path| (path.clone(), None)).collect();
            Delta {
                base,
                base_ids,
                here: BTreeMap::new(),
                sent,
            }
        };
        // a guard the ref could not take is the last copy of this instance, newer than the ref's
        let pending = self.pending()?;
        // a guard is kept beside the record at once where the ref is in a reftable, whose lock the
        // transaction this runs inside of holds
        let held_by_git = keep == Keep::Guard && self.refs_in_reftable();
        let mut failure = None;
        // asked once, before this look writes anything: an edit it copies is in the database after
        let mut known = None;
        for attempt in 0 .. ATTEMPTS {
            let last = self.backup_tip()?;
            let old = match pending.as_ref().or(last.as_ref()) {
                Some(at) => self.read_copy(at)?,
                None => Copy::default(),
            };
            let mut new = self.next_copy(&old, &delta, keep, &mut known)?;
            let written_over = new.lost.keys().cloned().collect();
            if new == old && pending.is_none() {
                return Ok((Look { written_over, copied: Ok(()) }, new));
            }
            let commit = match self.build_copy(last.as_ref(), pending.as_ref(), &old, &mut new, &delta) {
                Ok(commit) => commit,
                Err(e) => return Ok((Look { written_over, copied: Err(e) }, new)),
            };
            if held_by_git {
                let copied = self.keep_pending(&commit);
                return Ok((Look { written_over, copied }, new));
            }
            match self.move_backup_ref(last.as_ref(), &commit) {
                Ok(()) => {
                    if let Some(pending) = &pending {
                        self.drop_pending(pending);
                    }
                    return Ok((Look { written_over, copied: Ok(()) }, new));
                },
                // a guard is never given up: what the ref would not take is kept beside the record
                Err(_) if keep == Keep::Guard && attempt + 1 == ATTEMPTS => {
                    let copied = self.keep_pending(&commit);
                    return Ok((Look { written_over, copied }, new));
                },
                Err(e) => {
                    failure = Some((written_over, new, e));
                    pause(attempt);
                },
            }
        }
        let (written_over, new, e) = failure.expect("ATTEMPTS is above zero");
        Ok((Look { written_over, copied: Err(e) }, new))
    }

    /// The copy `old` becomes on this look.
    fn next_copy(&self, old: &Copy, delta: &Delta, keep: Keep, known: &mut Option<Known>) -> Result<Copy, StoreError> {
        let mut lost = old.lost.clone();
        let changed: Vec<(&String, &FileId)> =
            old.guard.iter().filter(|(path, id)| delta.here.get(*path) != Some(*id) && !lost.contains_key(*path)).collect();
        if !changed.is_empty() && known.is_none() {
            *known = Some(self.known(delta, &changed)?);
        }
        let none = Known {
            blobs:     BTreeSet::new(),
            git_wrote: BTreeSet::new(),
        };
        let known = known.as_ref().unwrap_or(&none);
        for (path, id) in changed {
            // a file git wrote is gone, holds bytes a commit brought, or is one git wrote through a
            // filter or over a guarded edit; one the person edited since holds new bytes
            let by_git = delta.here.get(path).is_none_or(|now| known.blobs.contains(&now.blob)) || known.git_wrote.contains(path);
            if by_git {
                lost.insert(path.clone(), id.clone());
            }
        }
        lost.retain(|path, id| delta.here.get(path) != Some(id));
        if keep == Keep::Accept {
            lost.clear();
        }
        let edit = delta.sent.iter().filter_map(|(path, id)| id.clone().map(|id| (path.clone(), id))).collect();
        let guard = if keep == Keep::Guard { delta.here.clone() } else { BTreeMap::new() };
        Ok(Copy {
            edit,
            guard,
            lost,
        })
    }

    /// The commit the backup ref is at, or `None` where there is none.
    fn backup_tip(&self) -> Result<Option<Oid>, StoreError> {
        let reference = self.backup_ref();
        let out = self.store.git.run(&["rev-parse", "--verify", "-q", &format!("{reference}^{{commit}}")], &[], None)?;
        match out.code {
            Some(0) => Oid::parse(&out.text()).map(Some),
            Some(1) => Ok(None),
            _ => {
                Err(StoreError::Git {
                    command: format!("rev-parse --verify {reference}"),
                    detail:  out.stderr,
                })
            },
        }
    }

    fn read_copy(&self, last: &Oid) -> Result<Copy, StoreError> {
        let key = self.record_key()?;
        let mut copy = Copy::default();
        for (path, id) in self.store.tree_ids_under(last, Some(&format!("{key}/")))? {
            let Some(rest) = path.strip_prefix(&format!("{key}/")) else { continue };
            let (set, rel) = rest.split_once('/').unwrap_or((rest, ""));
            let into = match set {
                "edit" => &mut copy.edit,
                "guard" => &mut copy.guard,
                "lost" => &mut copy.lost,
                _ => continue,
            };
            into.insert(rel.to_owned(), id);
        }
        Ok(copy)
    }

    /// Builds `new` as the commit that follows `last` in the backup ref, with `pending`, a copy the
    /// ref has not taken yet, as its second parent. The files whose bytes the object database may
    /// not hold (an edit or a guard's file that neither the base nor the last copy has) are written
    /// to it first, and their ids in `new` are the ids of what was written. Nothing here needs a
    /// lock on a ref.
    fn build_copy(&self, last: Option<&Oid>, pending: Option<&Oid>, old: &Copy, new: &mut Copy, delta: &Delta) -> Result<Oid, StoreError> {
        let git = &self.store.git;
        let held: BTreeSet<&Oid> = delta.base_ids.values().map(|id| &id.blob).chain(old.blobs()).collect();
        let unwritten: Vec<String> = new
            .edit
            .iter()
            .chain(new.guard.iter())
            .filter(|(_, id)| !held.contains(&id.blob))
            .map(|(path, _)| path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !unwritten.is_empty() {
            let input: String = unwritten.iter().map(|p| format!("{}\n", self.dir.join(p).display())).collect();
            let out = git.must(&["hash-object", "-w", "--no-filters", "--stdin-paths"], &[], Some(input.as_bytes()))?;
            let text = out.text();
            let blobs: Vec<&str> = text.lines().collect();
            if blobs.len() != unwritten.len() {
                return Err(StoreError::Git {
                    command: "hash-object -w --stdin-paths".to_owned(),
                    detail:  format!("{} ids for {} paths", blobs.len(), unwritten.len()),
                });
            }
            // the bytes written are the ones kept, should the file have changed since it was hashed
            for (path, blob) in unwritten.iter().zip(blobs) {
                let blob = Oid::parse(blob)?;
                for set in [&mut new.edit, &mut new.guard] {
                    if let Some(id) = set.get_mut(path) {
                        id.blob = blob.clone();
                    }
                }
            }
        }
        let key = self.record_key()?;
        let mine = if new.is_empty() { None } else { Some(self.key_tree(new)?) };
        let mut entries = String::new();
        if let Some(last) = last {
            let top = git.must(&["ls-tree", "-z", last.as_str()], &[], None)?;
            for entry in top.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
                let entry = String::from_utf8_lossy(entry);
                if entry.split_once('\t').is_none_or(|(_, name)| name != key) {
                    entries.push_str(&entry);
                    entries.push('\0');
                }
            }
        }
        if let Some(mine) = &mine {
            entries.push_str(&format!("040000 tree {mine}\t{key}\0"));
        }
        let tree = git.must(&["mktree", "-z"], &[], Some(entries.as_bytes()))?.text();
        let who = &self.store.identity;
        let ident = [
            ("GIT_AUTHOR_NAME", who.name.as_str()),
            ("GIT_AUTHOR_EMAIL", who.email.as_str()),
            ("GIT_COMMITTER_NAME", who.name.as_str()),
            ("GIT_COMMITTER_EMAIL", who.email.as_str()),
        ];
        let message = format!("backup of the files of an instance at {}", self.dir.display());
        let mut args = vec!["commit-tree", tree.as_str()];
        for parent in last.into_iter().chain(pending.filter(|p| Some(*p) != last)) {
            args.extend(["-p", parent.as_str()]);
        }
        args.extend(["-m", message.as_str()]);
        Oid::parse(&git.must(&args, &ident, None)?.text())
    }

    /// Moves the backup ref from `last` to `commit`. Fails without moving it where another writer
    /// moved it first or holds its lock, and the copy is then built again.
    fn move_backup_ref(&self, last: Option<&Oid>, commit: &Oid) -> Result<(), StoreError> {
        let reference = self.backup_ref();
        // an empty old value is the ref not existing, whatever the length of an id
        let old_value = last.map_or("", Oid::as_str);
        let out = self.store.git.run(&["update-ref", &reference, commit.as_str(), old_value], &[], None)?;
        if out.ok() {
            Ok(())
        } else {
            Err(StoreError::Git {
                command: format!("update-ref {reference}"),
                detail:  out.stderr,
            })
        }
    }

    /// The tree of `copy`, its sets as directories, built in an index of its own.
    fn key_tree(&self, copy: &Copy) -> Result<String, StoreError> {
        let mut lines = String::new();
        for (set, files) in copy.sets() {
            for (path, id) in files {
                lines.push_str(&format!("{} {}\t{set}/{path}\0", id.mode.git(), id.blob));
            }
        }
        let records = self.record_file()?;
        let dir = records.parent().map(PathBuf::from).unwrap_or_default();
        let index = dir.join(format!(
            "mockspace-backup-{}-{}.idx",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _guard = RemoveOnDrop(index.clone());
        let index_s = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", index_s.as_str())];
        let git = &self.store.git;
        git.must(&["update-index", "-z", "--add", "--index-info"], &env, Some(lines.as_bytes()))?;
        Ok(git.must(&["write-tree"], &env, None)?.text())
    }

    /// The name of this instance's directory in the backup ref: the name of its record.
    fn record_key(&self) -> Result<String, StoreError> {
        let file = self.record_file()?;
        Ok(file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
    }
}

/// Waits a little longer on each attempt, by an amount that differs between processes, so that two
/// writers that lost to each other do not meet again.
fn pause(attempt: u64) {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let jitter = u64::from(nanos ^ std::process::id()) % 20;
    std::thread::sleep(std::time::Duration::from_millis((attempt + 1) * (5 + jitter)));
}
