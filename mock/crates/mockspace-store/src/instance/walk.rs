//! Which files of the directory are the instance's: every regular file its patterns match, less
//! what a `.gitignore` keeps out and what a cache directory holds.
//!
//! A file a `.gitignore` of the work tree matches is not data, as it is not for a branch: it is
//! never read into an edit, so never committed, and never removed or written by the engine. The
//! rules are git's own, read by `git ls-files` from the directory, so a `.gitignore` above the
//! instance counts as one inside it does. The repository's `info/exclude` and `core.excludesFile`
//! are not read: the first holds the line that hides the instance itself, and the second is one
//! person's and not the repository's. A file the ref already holds stays held when a `.gitignore`
//! matches it later, as a tracked file stays tracked in git. A directory outside every work tree
//! has no `.gitignore` git would read, and none applies there.
//!
//! A cache directory (one with a `CACHEDIR.TAG` that carries the standard signature, which is what
//! cargo writes into a `target`) is a build's output and is not read either, unless the base holds
//! a file under it; it is not named as ignored, since nobody means it as data.

use std::collections::BTreeSet;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::Instance;
use crate::error::StoreError;

/// The files of the directory that are the instance's, and the paths a `.gitignore` keeps out of
/// it: a file, or a directory with a trailing `/` where git ignores the whole of it.
pub(super) struct Walk {
    pub(super) files:   Vec<String>,
    pub(super) ignored: Vec<String>,
}

/// What git ignores under the directory, by path from it: whole directories with a trailing `/`,
/// and single files.
struct Ignores {
    dirs:  BTreeSet<Vec<u8>>,
    files: BTreeSet<Vec<u8>>,
}

impl Instance<'_> {
    /// The regular files under the directory that are the instance's, as `/`-separated paths,
    /// sorted.
    pub(super) fn walk(&self) -> Result<Vec<String>, StoreError> {
        Ok(self.walk_all()?.files)
    }

    /// The paths of the directory a `.gitignore` keeps out of the instance, sorted: a file, or a
    /// directory with a trailing `/` git ignores whole. None where there is no directory.
    pub fn ignored(&self) -> Result<Vec<String>, StoreError> {
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        Ok(self.walk_all()?.ignored)
    }

    pub(super) fn walk_all(&self) -> Result<Walk, StoreError> {
        let known: BTreeSet<String> = match self.base()? {
            Some(base) => self.ids_of(&base)?.into_keys().collect(),
            None => match self.store.local_oid()? {
                Some(tip) => self.store.tree_ids(&tip)?.into_keys().collect(),
                None => BTreeSet::new(),
            },
        };
        let ignores = self.ignores()?;
        let mut walk = Walk {
            files:   Vec::new(),
            ignored: Vec::new(),
        };
        self.walk_into(&self.dir, "", b"", true, false, &known, &ignores, &mut walk)?;
        Ok(walk)
    }

    /// What git ignores under the directory by the `.gitignore` files of the work tree alone.
    fn ignores(&self) -> Result<Ignores, StoreError> {
        let git = self.store.git.in_dir(self.dir.clone());
        let args = ["ls-files", "-z", "--others", "--ignored", "--exclude-per-directory=.gitignore", "--directory", "--", "."];
        let out = git.run(&args, &[], None)?;
        if out.code == Some(128) && out.stderr.contains("not a git repository") {
            return Ok(Ignores {
                dirs:  BTreeSet::new(),
                files: BTreeSet::new(),
            });
        }
        if !out.ok() {
            return Err(StoreError::Git {
                command: args.join(" "),
                detail:  out.stderr,
            });
        }
        // by the bytes of each name, as git matched them: two names that read as one lossy text
        // get each its own verdict
        let listed: BTreeSet<Vec<u8>> = out.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()).map(<[u8]>::to_vec).collect();
        // With `--directory` git lists a directory that only holds ignored files and the files in it
        // as well; a directory it ignores whole is listed with nothing under it. Sorted, what is
        // under a directory comes right after it.
        let sorted: Vec<&Vec<u8>> = listed.iter().collect();
        let dirs = sorted
            .iter()
            .enumerate()
            .filter(|(i, e)| e.ends_with(b"/") && !sorted.get(i + 1).is_some_and(|n| n.starts_with(e)))
            .map(|(_, e)| (*e).clone())
            .collect();
        let files = listed.into_iter().filter(|e| !e.ends_with(b"/")).collect();
        Ok(Ignores {
            dirs,
            files,
        })
    }

    /// Whether the directory at `at` is a cache directory, by the signature the cache directory
    /// tag specification names.
    fn is_cache_dir(at: &Path) -> bool {
        use std::io::Read;
        const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";
        let mut head = [0u8; SIGNATURE.len()];
        std::fs::File::open(at.join("CACHEDIR.TAG")).and_then(|mut f| f.read_exact(&mut head)).is_ok() && head == *SIGNATURE
    }

    /// `text` says whether `prefix` is the path as it is on disk, and `raw_prefix` is its bytes. A
    /// name that is not UTF-8 is read lossily for the patterns and the cache tag, and by its bytes
    /// against what git ignores, so each name gets the verdict git gave it. It is refused, by that
    /// lossy name, only where the instance would read it: a ref names its files as text and cannot
    /// hold it.
    #[allow(clippy::too_many_arguments)]
    fn walk_into(
        &self,
        dir: &Path,
        prefix: &str,
        raw_prefix: &[u8],
        text: bool,
        under_ignored: bool,
        known: &BTreeSet<String>,
        ignores: &Ignores,
        out: &mut Walk,
    ) -> Result<(), StoreError> {
        let mut entries = std::fs::read_dir(dir)
            .map_err(|e| self.io("reading", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.io("reading", e))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let raw = entry.file_name();
            let text = text && raw.to_str().is_some();
            let name = raw.to_string_lossy();
            let rel = if prefix.is_empty() { name.into_owned() } else { format!("{prefix}/{name}") };
            let raw_rel = if raw_prefix.is_empty() { raw.as_bytes().to_vec() } else { [raw_prefix, b"/", raw.as_bytes()].concat() };
            let kind = entry.file_type().map_err(|e| self.io("reading", e))?;
            if kind.is_dir() {
                let below = format!("{rel}/");
                let holds_some = known.range(below.clone() ..).next().is_some_and(|k| k.starts_with(&below));
                let ignored = under_ignored || ignores.dirs.contains(&[raw_rel.as_slice(), b"/"].concat());
                if self.held.as_ref().is_some_and(|h| !h.may_hold_under(&rel)) {
                    // nothing of the patterns can be in there
                } else if !holds_some && Self::is_cache_dir(&entry.path()) {
                    // a build's output: not data, and not named, because nobody means it as data
                } else if ignored && !holds_some {
                    // a directory inside one ignored whole is reached only where that one holds
                    // files of the ref, and is not named by it, so it is named here
                    out.ignored.push(below);
                } else {
                    self.walk_into(&entry.path(), &rel, &raw_rel, text, ignored, known, ignores, out)?;
                }
            } else if !self.holds(&rel) {
                // the branch's own: not this instance's to read or to refuse
            } else if (under_ignored || ignores.files.contains(&raw_rel)) && !known.contains(&rel) {
                out.ignored.push(rel);
            } else if !text {
                return Err(self.fail("holds a file name that is not text, which a ref cannot hold; rename it", vec![rel]));
            } else if kind.is_symlink() {
                return Err(self.fail("holds a symlink", vec![rel]));
            } else if kind.is_file() {
                out.files.push(rel);
            } else {
                return Err(self.fail("holds something that is not a regular file", vec![rel]));
            }
        }
        Ok(())
    }
}
