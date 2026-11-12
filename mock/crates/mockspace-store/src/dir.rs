//! The directory backend: files under a root, no history, no remote.

use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::edit::Edit;
use crate::error::StoreError;
use crate::merge::MergeHook;
use crate::path::{under, validate_path};
use crate::reader::Reader;
use crate::store::{Applied, Freshness, Store};

/// A file being written, beside its target; never listed.
pub(crate) const TMP_PREFIX: &str = ".mockspace-store-tmp-";
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A store that is a directory. The merge hook is never called: a directory has
/// one state, and whoever shares it shares the filesystem's own ordering. An edit is
/// validated whole before anything is written, but the files are then written one
/// by one, each by rename: an I/O failure part way leaves the earlier files in place.
///
/// A symlinked directory is walked into and a symlinked file is written through, as the walk and
/// the `fs::write` of the registry did before it used a store. The test history wrote a temporary
/// file and renamed it over the path, which replaced a link; through a store it writes through one.
#[derive(Debug, Clone)]
pub struct DirStore {
    root: PathBuf,
}

impl DirStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
        }
    }

    fn file(&self, path: &str) -> PathBuf {
        path.split('/')
            .fold(self.root.clone(), |p, part| p.join(part))
    }
}

fn io_err(context: String) -> impl FnOnce(io::Error) -> StoreError {
    move |source| {
        StoreError::Io {
            context,
            source,
        }
    }
}

/// Lists the files below `dir`. A symlinked directory is walked into, as the walk of the registry
/// has always done, and one that leads back to a directory already being walked is passed over,
/// so a link to an ancestor ends the listing rather than looping it. A link that leads nowhere is
/// listed like any entry that is not a directory, and reads as absent.
fn walk(
    root: &Path,
    dir: &Path,
    on_path: &mut Vec<PathBuf>,
    out: &mut Vec<String>,
) -> Result<(), StoreError> {
    let here = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if on_path.contains(&here) {
        return Ok(());
    }
    on_path.push(here);
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound && dir == root => return Ok(()),
        Err(e) => return Err(io_err(format!("reading `{}`", dir.display()))(e)),
    };
    for entry in entries {
        let entry = entry.map_err(io_err(format!("reading `{}`", dir.display())))?;
        if entry.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
            continue;
        }
        let path = entry.path();
        // `metadata` follows a link, which `file_type` of the entry does not
        if fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            walk(root, &path, on_path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            // a store names its files as text; a lossy name would name no file and read as absent
            let parts = rel
                .components()
                .map(|c| {
                    c.as_os_str().to_str().map(str::to_owned).ok_or_else(|| {
                        StoreError::InvalidPath {
                            path: rel.to_string_lossy().into_owned(),
                            why:  "is not text, which a store cannot name; rename it",
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            out.push(parts.join("/"));
        }
    }
    on_path.pop();
    Ok(())
}

impl Reader for DirStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        validate_path(path)?;
        let file = self.file(path);
        match fs::read(&file) {
            Ok(b) => Ok(Some(b)),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::IsADirectory
                ) =>
            {
                Ok(None)
            },
            Err(e) => Err(io_err(format!("reading `{}`", file.display()))(e)),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let mut all = Vec::new();
        walk(&self.root, &self.root, &mut Vec::new(), &mut all)?;
        all.retain(|p| under(p, prefix));
        all.sort();
        Ok(all)
    }
}

impl Store for DirStore {
    fn refresh(&self) -> Result<Freshness, StoreError> {
        Ok(Freshness::Local)
    }

    fn apply(
        &self,
        edit: &Edit,
        _message: &str,
        _hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError> {
        edit.validate()?;
        edit.check_against(self)?;
        let mut changed = false;
        // removals first, so a file can take the place of a directory it empties
        let removals = edit.entries().filter(|e| e.1.is_none());
        let puts = edit.entries().filter(|e| e.1.is_some());
        for (path, bytes) in removals.chain(puts) {
            let file = self.file(path);
            // a symlinked file is written through to what it points at, as `fs::write` did, and
            // the link stays; removal takes the link itself
            let link = file.clone();
            let file = if bytes.is_some() { through_links(&file) } else { file };
            match bytes {
                Some(bytes) => {
                    if self.read(path)?.as_deref() == Some(bytes) {
                        continue;
                    }
                    // directories are made for the store's own path, never for where a link leads, which
                    // `fs::write` did not make either
                    if let Some(parent) = link.parent() {
                        fs::create_dir_all(parent)
                            .map_err(io_err(format!("creating `{}`", parent.display())))?;
                    }
                    // written beside the target and renamed over it, so a reader never sees half a file
                    let tmp = file.with_file_name(format!(
                        "{TMP_PREFIX}{}-{}",
                        std::process::id(),
                        TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    ));
                    fs::write(&tmp, bytes)
                        .map_err(io_err(format!("writing `{}`", tmp.display())))?;
                    fs::rename(&tmp, &file)
                        .map_err(io_err(format!("renaming into `{}`", file.display())))?;
                    changed = true;
                },
                None => {
                    match fs::remove_file(&file) {
                        Ok(()) => {
                            changed = true;
                            prune_empty_dirs(&self.root, file.parent());
                        },
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::NotFound
                                    | io::ErrorKind::NotADirectory
                                    | io::ErrorKind::IsADirectory
                            ) => {},
                        Err(e) => return Err(io_err(format!("removing `{}`", file.display()))(e)),
                    }
                },
            }
        }
        Ok(Applied {
            commit: None,
            changed,
            attempts: 1,
            confirmed: true,
        })
    }
}

/// `file` with each symlink it is, one after another, replaced by what it points at (a link that
/// points at nothing yet by where it points, which a write then creates if its directory is there). A path that is not a
/// link, or a chain longer than any filesystem follows, comes back as it was, and the write that
/// follows it then fails as the read before it does.
fn through_links(file: &Path) -> PathBuf {
    let mut at = file.to_path_buf();
    for _ in 0 .. 40 {
        match fs::read_link(&at) {
            Ok(to) => {
                at = match at.parent() {
                    Some(parent) => parent.join(to),
                    None => to,
                };
            },
            Err(_) => return at,
        }
    }
    file.to_path_buf()
}

fn prune_empty_dirs(root: &Path, mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if d == root || fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}
