//! The repository's index: every document it has written, read once and kept.
//!
//! The query tools used to walk the research tree on every run and score words by counting them.
//! That is a minute of work to say that five copies of one note outrank the design round that
//! holds the answer. The index reads each file once, keyed by its path and the hash of its
//! content, keeps the text in a full-text table that ranks by term and document frequency, keeps
//! a fingerprint so copies count once, and keeps each document's date and who wrote it. A warm
//! query touches no file but the ones that changed.
//!
//! It lives in the user cache (`$XDG_CACHE_HOME/mockspace/index/<repo key>.sqlite`), is never
//! committed, and is rebuilt from the tree whenever it is missing, unreadable or from another
//! schema, so it holds nothing that is not on disk.

pub mod engine;
mod ingest;
mod search;
pub mod shingle;
pub mod stamp;
mod walk;

use std::path::{Path, PathBuf};

use mockspace_lint_rules::DocClass;
use rusqlite::Connection;

pub use ingest::Refreshed;
pub use walk::Source;

/// Bumped whenever the tables change shape. An index of another version is dropped and rebuilt,
/// never migrated: it is a cache.
const SCHEMA: i64 = 1;

/// The index of one repository.
pub struct Index {
    conn:    Connection,
    /// What the last refresh could not read, one sentence each, shown by every search.
    caveats: Vec<String>,
}

/// Why an index could not be opened.
#[derive(Debug)]
pub struct IndexError(pub String);

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        Self(e.to_string())
    }
}

/// Where the user cache is: `$XDG_CACHE_HOME`, else `~/.cache`.
#[must_use]
pub fn cache_root() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"));
    xdg.filter(|p| p.is_absolute()).or(home)
}

/// The file an index of the repository at `repo_root` is kept in, under `cache`.
///
/// Named for the directory and a digest of its whole path, so two clones of one repository, or
/// two repositories with one name, never share a file.
#[must_use]
pub fn index_path(cache: &Path, repo_root: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let canon = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let digest = Sha256::digest(canon.to_string_lossy().as_bytes());
    let short: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let name = canon
        .file_name()
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().to_string());
    cache
        .join("mockspace")
        .join("index")
        .join(format!("{name}-{short}.sqlite"))
}

impl Index {
    /// Open the index at `path`, creating it, or dropping and recreating one that is of another
    /// schema or cannot be read.
    ///
    /// # Errors
    /// When the file cannot be created even after a drop.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| IndexError(format!("{}: {e}", dir.display())))?;
        }
        match Self::try_open(Connection::open(path)) {
            Ok(i) => Ok(i),
            Err(_) => {
                // A cache that cannot be read is a cache that is gone.
                let _ = std::fs::remove_file(path);
                Self::try_open(Connection::open(path))
            },
        }
    }

    /// An index held in memory, for a test or a run with no cache to write to.
    ///
    /// # Errors
    /// When SQLite cannot start.
    pub fn in_memory() -> Result<Self, IndexError> {
        Self::try_open(Connection::open_in_memory())
    }

    fn try_open(conn: rusqlite::Result<Connection>) -> Result<Self, IndexError> {
        let conn = conn?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        let have: Option<i64> = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .ok();
        if have != Some(SCHEMA) {
            conn.execute_batch(
                "DROP TABLE IF EXISTS doc_fts; DROP TABLE IF EXISTS doc;
                 CREATE TABLE doc (
                     id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, file TEXT NOT NULL,
                     class TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
                     hash TEXT NOT NULL, day INTEGER, words INTEGER NOT NULL,
                     sketch BLOB NOT NULL, held INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE VIRTUAL TABLE doc_fts USING fts5(
                     heads, body, tokenize = 'porter unicode61'
                 );",
            )?;
            conn.pragma_update(None, "user_version", SCHEMA)?;
        }
        Ok(Self {
            conn,
            caveats: Vec::new(),
        })
    }

    /// How many documents the index holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.conn
            .query_row("SELECT count(*) FROM doc", [], |r| r.get::<_, i64>(0))
            .map_or(0, |n| usize::try_from(n).unwrap_or(0))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The class a path was indexed as, for a test and for `mock` reports.
    #[must_use]
    pub fn class_of(&self, path: &str) -> Option<DocClass> {
        self.conn
            .query_row("SELECT class FROM doc WHERE path = ?1", [path], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|l| DocClass::from_label(&l))
    }
}

#[cfg(test)]
mod tests;
