//! The directory backend: files under a root, no history, no remote.

use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::edit::Edit;
use crate::error::StoreError;
use crate::merge::MergeHook;
use crate::path::{under, validate_path};
use crate::reader::Reader;
use crate::store::{Applied, Freshness, Store};

/// A store that is a directory. The merge hook is never called: a directory has
/// one state, and whoever shares it shares the filesystem's own ordering.
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

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), StoreError> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound && dir == root => return Ok(()),
        Err(e) => return Err(io_err(format!("reading `{}`", dir.display()))(e)),
    };
    for entry in entries {
        let entry = entry.map_err(io_err(format!("reading `{}`", dir.display())))?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(io_err(format!("stat `{}`", path.display())))?
            .is_dir()
        {
            walk(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
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
        walk(&self.root, &self.root, &mut all)?;
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
        let mut changed = false;
        for (path, bytes) in edit.entries() {
            let file = self.file(path);
            match bytes {
                Some(bytes) => {
                    if self.read(path)?.as_deref() == Some(bytes) {
                        continue;
                    }
                    if let Some(parent) = file.parent() {
                        fs::create_dir_all(parent)
                            .map_err(io_err(format!("creating `{}`", parent.display())))?;
                    }
                    // written beside the target and renamed over it, so a reader never sees half a file
                    let tmp = file.with_extension(format!("tmp-{}", std::process::id()));
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
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {},
                        Err(e) => return Err(io_err(format!("removing `{}`", file.display()))(e)),
                    }
                },
            }
        }
        Ok(Applied {
            commit: None,
            changed,
            attempts: 1,
        })
    }
}

fn prune_empty_dirs(root: &Path, mut dir: Option<&Path>) {
    while let Some(d) = dir {
        if d == root || fs::remove_dir(d).is_err() {
            return;
        }
        dir = d.parent();
    }
}
