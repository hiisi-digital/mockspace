//! Where an instance keeps its base, and the exclude line that hides its directory.
//!
//! Each instance has a file of its own in the repository's common directory, named by the git
//! id of the pair of its local ref and its directory, so two instances made at the same moment
//! write different files and nothing is read and written back as a whole. A record is replaced
//! by writing a staged file of a unique name and renaming it onto the record.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Instance;
use crate::error::StoreError;
use crate::ids::Base;

/// The directory of the common git directory that holds one record for each instance.
const RECORDS: &str = "mock-instances";

/// Makes the name of each staged file different from every other of this process.
static STAGED: AtomicU64 = AtomicU64::new(0);

impl Instance<'_> {
    /// The base this instance was last brought to, or `None` when no record exists. A record
    /// that names another ref or directory is an error: the name is an id of the pair, so it
    /// means the file was changed by hand.
    pub(super) fn read_record(&self) -> Result<Option<Base>, StoreError> {
        let text = match std::fs::read_to_string(self.record_file()?) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.io("reading the record of", e)),
        };
        let mut fields = text.trim_end_matches('\n').splitn(3, '\t');
        match (fields.next(), fields.next(), fields.next()) {
            (Some(base), Some(local), Some(dir))
                if local == self.store.pair().local() && dir == self.dir_text() =>
            {
                Base::from_record(base).map(Some).ok_or_else(|| self.fail("has a record whose base is no commit", vec![base.to_owned()]))
            },
            _ => Err(self.fail("has a record that is not its own", Vec::new())),
        }
    }

    /// Records `base` as the commit the files of the directory were brought to.
    pub(super) fn record(&self, base: &Base) -> Result<(), StoreError> {
        let file = self.record_file()?;
        let Some(parent) = file.parent() else { return Ok(()) };
        std::fs::create_dir_all(parent).map_err(|e| self.io("creating the records of", e))?;
        let line = format!(
            "{}\t{}\t{}\n",
            base.record_text(),
            self.store.pair().local(),
            self.dir_text()
        );
        let staged = file.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ));
        let done = std::fs::write(&staged, line).and_then(|()| std::fs::rename(&staged, &file));
        if done.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        done.map_err(|e| self.io("writing the record of", e))
    }

    /// Drops the record, if there is one. Used to undo a `materialize` that did not finish.
    pub(super) fn forget(&self) {
        self.forget_stats();
        if let Ok(file) = self.record_file() {
            let _ = std::fs::remove_file(file);
        }
    }

    pub(super) fn record_file(&self) -> Result<PathBuf, StoreError> {
        if let Some(file) = self.record_at.get() {
            return Ok(file.clone());
        }
        let common = self.store.git.must(
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            &[],
            None,
        )?;
        let key = format!("{}\0{}", self.store.pair().local(), self.dir_text());
        let id = self
            .store
            .git
            .must(&["hash-object", "--stdin"], &[], Some(key.as_bytes()))?;
        let file = PathBuf::from(common.text()).join(RECORDS).join(id.text());
        let _ = self.record_at.set(file.clone());
        Ok(file)
    }

    /// Adds the directory to the exclude file the repository shares between its work trees, when
    /// it lies inside the work tree this store was opened on. The line is appended, so two
    /// instances made together both keep theirs.
    pub(super) fn exclude(&self) -> Result<(), StoreError> {
        // a failed call is an error, never read as lying in no work tree: the line would be left
        // out, and the files of the instance would read as new on the branch
        let Some(top) = self.store.git.work_tree_top()? else { return Ok(()) };
        let (Ok(top), Ok(dir)) = (
            std::fs::canonicalize(top),
            std::fs::canonicalize(&self.dir),
        ) else {
            return Ok(());
        };
        let Ok(rel) = dir.strip_prefix(&top) else { return Ok(()) };
        let Some(rel) = rel.to_str() else { return Ok(()) };
        if rel.is_empty() && self.held.is_none() {
            return Ok(());
        }
        // the whole directory, or each pattern under it: a pattern is checked to hold nothing the
        // exclude file reads otherwise than the instance does
        let lines = super::lines::exclude_lines(rel, self.held.as_ref());
        let out = self.store.git.must(
            &["rev-parse", "--path-format=absolute", "--git-path", "info/exclude"],
            &[],
            None,
        )?;
        let file = PathBuf::from(out.text());
        let held = std::fs::read_to_string(&file).unwrap_or_default();
        let missing: Vec<&String> = lines.iter().filter(|line| !held.lines().any(|l| l == line.as_str())).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let line = missing.iter().map(|l| l.as_str()).collect::<Vec<_>>().join("\n");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| self.io("creating the directory of", e))?;
        }
        let lead = if held.is_empty() || held.ends_with('\n') { "" } else { "\n" };
        let mut handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(|e| self.io("opening the exclude file for", e))?;
        std::io::Write::write_all(&mut handle, format!("{lead}{line}\n").as_bytes())
            .map_err(|e| self.io("writing the exclude file for", e))
    }
}
