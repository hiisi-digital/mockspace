//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Data kept on a ref of its own: `refs/mock/<name>` holds a tree of files, with history, apart
//! from the branches.
//!
//! A ref is shared by every branch and every worktree of a clone, so it holds only data whose
//! meaning does not depend on the branch. It is not a branch: no branch list shows it and a
//! plain push or clone does not carry it, which is why [`DataRef::fetch`] and [`DataRef::push`]
//! name it in a refspec every time.
//!
//! A ref is worked on through an instance, a directory that holds its files. The instance is
//! mapped by its own index file inside the git directory and the directory as the work tree, so
//! the branch's index and status never see it, and it is excluded from the status of the branch.
//! Every call that reads or writes an index names the index file, and none lets an ambient
//! `GIT_INDEX_FILE` reach git, because a `read-tree` that finds it unset writes the ref's tree into
//! the branch's own index. A commit to the ref is made by `commit-tree` onto the tip and moved
//! with `update-ref` holding the old tip as its expectation, so two writers cannot both win.
//!
//! An instance holds changes nowhere else has until [`DataRef::commit`] runs, and `git clean
//! -fdx` removes an excluded directory with them. The callers commit on a short cycle for that
//! reason.
//!
//! Everything shells out to git, as the rest of the engine does.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pairing::{self, Candidate, Pairing, Resolution, TreeHasher};

/// The namespace the refs live in, which is not `refs/heads`.
pub const NAMESPACE: &str = "refs/mock";

/// Where a fetched copy of a remote's ref is held while it is compared with the local one.
const REMOTE_NAMESPACE: &str = "refs/mock-remote";

/// What a fetch found, compared with the local ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Synced {
    /// There was no local ref and it is now the remote's.
    Created,
    /// Both are at the same commit.
    UpToDate,
    /// The remote is ahead and the local ref moved up to it.
    FastForwarded,
    /// The local ref is ahead of the remote's: there is something to push.
    Ahead,
    /// Each has commits the other has not. Nothing was moved; the caller merges.
    Diverged,
}

/// What a push came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pushed {
    /// The remote has the local tip.
    Landed,
    /// The remote's ref moved since the last fetch, so this was not a fast-forward. Nothing was
    /// changed at the remote; fetch, merge and push again.
    Raced,
}

/// One data ref of one repository.
#[derive(Debug, Clone)]
pub struct DataRef {
    repo: PathBuf,
    name: String,
}

impl DataRef {
    /// The ref `refs/mock/<name>` of the repository at `repo`.
    ///
    /// # Errors
    ///
    /// When the name is not lowercase letters, digits, `_` and `-` led by a letter or a digit.
    pub fn new(repo: impl Into<PathBuf>, name: &str) -> Result<Self, String> {
        if !pairing::is_ref_name(name) {
            return Err(format!(
                "`{name}` is not a data ref name: lowercase letters, digits, `_` and `-`, led by a \
                 letter or a digit"
            ));
        }
        let repo = std::path::absolute(repo.into()).map_err(|e| e.to_string())?;
        Ok(Self {
            repo,
            name: name.to_owned(),
        })
    }

    /// The full ref name.
    pub fn ref_name(&self) -> String {
        format!("{NAMESPACE}/{}", self.name)
    }

    /// The commit the ref is at, or nothing where it does not exist.
    ///
    /// # Errors
    ///
    /// When git cannot be run.
    pub fn tip(&self) -> Result<Option<String>, String> {
        let out = self.git(
            &self.repo,
            &["rev-parse", "--verify", "-q", &self.ref_name()],
            None,
        )?;
        Ok(out.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()))
    }

    /// Makes the ref from a tree of this repository, as a commit with no parent. `tree` is
    /// anything git reads as a tree, such as `HEAD:mock/registry`. Refused where the ref exists.
    ///
    /// # Errors
    ///
    /// When the ref exists, the tree is not one, or git fails.
    pub fn seed(&self, tree: &str, message: &str) -> Result<String, String> {
        if self.tip()?.is_some() {
            return Err(format!(
                "`{}` exists, and is not seeded over",
                self.ref_name()
            ));
        }
        // `<rev>:<path>` takes everything after the colon as the path, so a peel cannot be
        // spelled on the end of it: resolve the spec first, peel the object it names second.
        let object = self.run(&["rev-parse", "--verify", tree])?;
        let tree_id =
            self.run(&["rev-parse", "--verify", &format!("{}^{{tree}}", object.trim())])?;
        let commit = self.run(&["commit-tree", "-m", message, tree_id.trim()])?;
        let zero = "0".repeat(commit.trim().len());
        self.run(&["update-ref", &self.ref_name(), commit.trim(), &zero])?;
        Ok(commit.trim().to_owned())
    }

    /// Writes the ref's files into `dir`, which is made if it is not there and has to be empty
    /// if it is, and excludes `dir` from the status of the repository where it sits inside it.
    ///
    /// # Errors
    ///
    /// When the ref does not exist, `dir` holds something, or git fails.
    pub fn instantiate(&self, dir: &Path) -> Result<(), String> {
        let dir = self.absolute(dir);
        if self.tip()?.is_none() {
            return Err(format!(
                "`{}` does not exist to be instantiated",
                self.ref_name()
            ));
        }
        if dir.exists()
            && std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err(format!(
                "`{}` holds files, and an instance is made in an empty directory",
                dir.display()
            ));
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.exclude(&dir)?;
        let _ = std::fs::remove_file(self.index_file()?);
        self.indexed(&dir, &["read-tree", &self.ref_name()])?;
        self.indexed(&dir, &["checkout-index", "-a"])?;
        Ok(())
    }

    /// Commits what `dir` holds onto the ref, or nothing where it holds what the tip holds.
    /// Returns the new commit. The ref moves only if it is still at the tip this started from.
    ///
    /// # Errors
    ///
    /// When the ref does not exist, another writer moved it meanwhile, or git fails.
    pub fn commit(&self, dir: &Path, message: &str) -> Result<Option<String>, String> {
        let Some(old) = self.tip()? else {
            return Err(format!(
                "`{}` does not exist to be committed to",
                self.ref_name()
            ));
        };
        self.commit_expecting(dir, message, &old)
    }

    /// [`commit`](Self::commit) holding `expected` as the tip: refused if the ref is not there.
    pub(crate) fn commit_expecting(
        &self,
        dir: &Path,
        message: &str,
        expected: &str,
    ) -> Result<Option<String>, String> {
        let dir = self.absolute(dir);
        self.indexed(&dir, &["add", "-A", "."])?;
        let tree = self.indexed(&dir, &["write-tree"])?;
        let held = self.run(&["rev-parse", &format!("{expected}^{{tree}}")])?;
        if tree.trim() == held.trim() {
            return Ok(None);
        }
        let new = self.run(&["commit-tree", "-p", expected, "-m", message, tree.trim()])?;
        self.run(&["update-ref", &self.ref_name(), new.trim(), expected])?;
        Ok(Some(new.trim().to_owned()))
    }

    /// Whether `dir` differs from the tip of the ref: the instance holds changes nowhere else has.
    ///
    /// # Errors
    ///
    /// When the ref does not exist or git fails.
    pub fn dirty(&self, dir: &Path) -> Result<bool, String> {
        let dir = self.absolute(dir);
        let Some(tip) = self.tip()? else {
            return Err(format!("`{}` does not exist", self.ref_name()));
        };
        self.indexed(&dir, &["add", "-A", "."])?;
        let tree = self.indexed(&dir, &["write-tree"])?;
        let held = self.run(&["rev-parse", &format!("{tip}^{{tree}}")])?;
        Ok(tree.trim() != held.trim())
    }

    /// Brings the remote's ref and compares it with the local one, moving the local one only
    /// forward. `depth` limits the history fetched, and is used only where there is no local ref,
    /// since a shallow remote tip cannot be compared with a local history it does not reach.
    ///
    /// # Errors
    ///
    /// When the remote has no such ref, which is said and never read as an empty ref, or git fails.
    pub fn fetch(&self, remote: &str, depth: Option<u32>) -> Result<Synced, String> {
        let local = self.tip()?;
        let tracking = format!("{REMOTE_NAMESPACE}/{remote}/{}", self.name);
        let spec = format!("+{}:{tracking}", self.ref_name());
        let mut args = vec!["fetch", "-q"];
        let depth_arg;
        if let (Some(d), None) = (depth, &local) {
            depth_arg = format!("--depth={d}");
            args.push(&depth_arg);
        }
        args.push(remote);
        args.push(&spec);
        // git says which ref the remote lacks, and that is passed on whole: an empty ref and a
        // missing one are not the same answer
        self.git(&self.repo, &args, None)?;
        let theirs = self.run(&["rev-parse", &tracking])?.trim().to_owned();
        let Some(ours) = local else {
            self.run(&["update-ref", &self.ref_name(), &theirs, &"0".repeat(theirs.len())])?;
            return Ok(Synced::Created);
        };
        if ours == theirs {
            return Ok(Synced::UpToDate);
        }
        if self.is_ancestor(&ours, &theirs)? {
            self.run(&["update-ref", &self.ref_name(), &theirs, &ours])?;
            return Ok(Synced::FastForwarded);
        }
        if self.is_ancestor(&theirs, &ours)? {
            return Ok(Synced::Ahead);
        }
        Ok(Synced::Diverged)
    }

    /// Pushes the local ref to the remote. It is a fast-forward or it is refused, and the remote
    /// is never overwritten.
    ///
    /// # Errors
    ///
    /// When the ref does not exist, or the push fails for a reason other than a lost race.
    pub fn push(&self, remote: &str) -> Result<Pushed, String> {
        let spec = format!("{0}:{0}", self.ref_name());
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["-c", "push.negotiate=false", "push", "-q", remote, &spec])
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(Pushed::Landed);
        }
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        if said.contains("[rejected]") || said.contains("non-fast-forward") {
            return Ok(Pushed::Raced);
        }
        Err(format!(
            "pushing `{}` to `{remote}`: {}",
            self.ref_name(),
            said.trim()
        ))
    }

    /// The pairing of a commit of the ref, or of the tip: its hash, its time and the digest of
    /// its files.
    ///
    /// # Errors
    ///
    /// When the commit is not there, or git fails.
    pub fn pairing_of(&self, rev: Option<&str>) -> Result<Pairing, String> {
        let rev = match rev {
            Some(r) => r.to_owned(),
            None => {
                self.tip()?
                    .ok_or_else(|| format!("`{}` does not exist", self.ref_name()))?
            },
        };
        let hash = self.run(&["rev-parse", "--verify", &format!("{rev}^{{commit}}")])?;
        let time = self.run(&["log", "-1", "--format=%ct", hash.trim()])?;
        Ok(Pairing {
            hash:   hash.trim().to_owned(),
            time:   time
                .trim()
                .parse::<i64>()
                .map_err(|e| format!("a commit time: {e}"))?,
            digest: self.digest_of(hash.trim())?,
        })
    }

    /// Which commit of the ref a pairing names: by its hash where the hash is there, and by the
    /// time and the digest where the repository was rehoused and the hash is gone.
    ///
    /// # Errors
    ///
    /// When the ref does not exist or git fails.
    pub fn resolve(&self, pairing: &Pairing) -> Result<Resolution, String> {
        let log = self.run(&["log", "--format=%H %ct", &self.ref_name()])?;
        let mut commits: Vec<(String, i64)> = Vec::new();
        for line in log.lines() {
            let (hash, time) = line.split_once(' ').ok_or("a log line with no time")?;
            commits.push((hash.to_owned(), time.parse().map_err(|_| "a commit time")?));
        }
        let candidates: Vec<Candidate<'_>> = commits
            .iter()
            .map(|(h, t)| {
                Candidate {
                    hash: h,
                    time: *t,
                }
            })
            .collect();
        Ok(pairing::resolve(
            pairing,
            &candidates,
            pairing::DEFAULT_WINDOW_SECS,
            |hash| self.digest_of(hash).ok(),
        ))
    }

    /// The digest of the files of a commit, read a file at a time.
    fn digest_of(&self, commit: &str) -> Result<String, String> {
        let listing = self.git(
            &self.repo,
            &["ls-tree", "-r", "-z", "--full-tree", commit],
            None,
        )?;
        let listing = listing.ok_or_else(|| format!("`{commit}` is not a commit here"))?;
        let mut files: Vec<(String, String)> = Vec::new();
        for entry in listing.split('\0').filter(|e| !e.is_empty()) {
            let (meta, path) = entry
                .split_once('\t')
                .ok_or("an ls-tree line with no path")?;
            let mut parts = meta.split(' ');
            let (_mode, kind, id) = (parts.next(), parts.next(), parts.next());
            match (kind, id) {
                (Some("blob"), Some(id)) => files.push((path.to_owned(), id.to_owned())),
                _ => {
                    return Err(format!(
                        "`{path}` is not a file: a data ref holds files only"
                    ));
                },
            }
        }
        files.sort();
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["cat-file", "--batch"])
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let ids: Vec<&str> = files.iter().map(|(_, id)| id.as_str()).collect();
        let mut hasher = TreeHasher::new();
        let read = std::thread::scope(|scope| -> Result<(), String> {
            scope.spawn(move || {
                for id in ids {
                    if writeln!(stdin, "{id}").is_err() {
                        break;
                    }
                }
            });
            let mut reader = BufReader::new(stdout);
            for (path, id) in &files {
                let mut header = String::new();
                reader.read_line(&mut header).map_err(|e| e.to_string())?;
                let size: usize = header
                    .split(' ')
                    .nth(2)
                    .and_then(|s| s.trim().parse().ok())
                    .ok_or_else(|| format!("cat-file said `{}` of {id}", header.trim()))?;
                let mut content = vec![0u8; size];
                reader.read_exact(&mut content).map_err(|e| e.to_string())?;
                let mut newline = [0u8; 1];
                reader.read_exact(&mut newline).map_err(|e| e.to_string())?;
                hasher.file(path, &content)?;
            }
            Ok(())
        });
        let _ = child.wait();
        read?;
        Ok(hasher.finish())
    }

    fn is_ancestor(&self, ancestor: &str, of: &str) -> Result<bool, String> {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["merge-base", "--is-ancestor", ancestor, of])
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .status()
            .map_err(|e| e.to_string())?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            other => Err(format!("merge-base --is-ancestor ended with {other:?}")),
        }
    }

    /// The index file the instance is mapped by, in the git directory so no worktree shares it.
    fn index_file(&self) -> Result<PathBuf, String> {
        let dir = self.run(&["rev-parse", "--absolute-git-dir"])?;
        Ok(PathBuf::from(dir.trim()).join(format!("mock-{}.index", self.name)))
    }

    /// Adds `dir` to the repository's own exclude file when it sits inside the repository.
    fn exclude(&self, dir: &Path) -> Result<(), String> {
        let Ok(rel) = dir.strip_prefix(self.absolute(Path::new(""))) else {
            return Ok(());
        };
        let line = format!("/{}/", rel.display());
        let git_dir = PathBuf::from(self.run(&["rev-parse", "--absolute-git-dir"])?.trim());
        let file = git_dir.join("info").join("exclude");
        let held = std::fs::read_to_string(&file).unwrap_or_default();
        if held.lines().any(|l| l == line) {
            return Ok(());
        }
        std::fs::create_dir_all(git_dir.join("info")).map_err(|e| e.to_string())?;
        let sep = if held.is_empty() || held.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&file, format!("{held}{sep}{line}\n")).map_err(|e| e.to_string())
    }

    fn absolute(&self, dir: &Path) -> PathBuf {
        if dir.is_absolute() { dir.to_path_buf() } else { self.repo.join(dir) }
    }

    /// Runs git at the repository and gives its output, failing with git's words.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.git(&self.repo, args, None)?
            .ok_or_else(|| format!("git {} found nothing", args.join(" ")))
    }

    /// Runs git with `dir` as the work tree and this ref's own index file. The branch's index is
    /// never named, and an ambient one is removed from the environment.
    fn indexed(&self, dir: &Path, args: &[&str]) -> Result<String, String> {
        let index = self.index_file()?;
        let git_dir = self.run(&["rev-parse", "--absolute-git-dir"])?;
        let git_dir_arg = format!("--git-dir={}", git_dir.trim());
        let work_arg = format!("--work-tree={}", dir.display());
        let mut all = vec![git_dir_arg.as_str(), work_arg.as_str()];
        all.extend_from_slice(args);
        self.git_raw(dir, &all, Some(&index))?
            .ok_or_else(|| format!("git {} found nothing", args.join(" ")))
    }

    fn git(
        &self,
        cwd: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let mut all = vec!["-C", cwd.to_str().ok_or("a path that is not text")?];
        all.extend_from_slice(args);
        self.git_raw(cwd, &all, index)
    }

    /// Nothing for a command that exits 1 with no output (`rev-parse -q` of a missing ref),
    /// the output of one that succeeds, the error of the rest.
    fn git_raw(
        &self,
        cwd: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE");
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("git could not be run: {e}"))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(Some(stdout));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.code() == Some(1) && stdout.is_empty() && stderr.trim().is_empty() {
            return Ok(None);
        }
        Err(format!("git {}: {}", args.join(" "), stderr.trim()))
    }
}

#[cfg(test)]
#[path = "data_refs_tests.rs"]
mod tests;
