//! The contract: a store reads like a directory and writes like a log.

use crate::edit::Edit;
use crate::error::StoreError;
use crate::merge::MergeHook;
use crate::reader::Reader;

/// What a refresh found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// The remote was asked and answered with this tip.
    Fetched {
        tip: String,
    },
    /// The remote was asked and has no such ref yet. The store is empty, and
    /// the answer comes from the remote, not from a missing local file.
    Empty,
    /// A backend with nothing to fetch from, such as a directory.
    Local,
}

/// What a write did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The commit now at the tip, for a backend that keeps history. `None` for
    /// a directory, and for a ref that is still empty after an empty edit.
    pub commit:   Option<String>,
    /// False when the edit changed nothing and no commit was written.
    pub changed:  bool,
    /// Push attempts it took, 1 when nobody else was writing.
    pub attempts: u32,
}

/// A place the data of one namespace lives: a directory, or a git ref.
///
/// Reads see the state of the last refresh (a directory has only the current
/// state). A write refreshes first, writes one commit holding the whole edit,
/// and on a ref pushes it with a lease. All of it is plain git and files.
pub trait Store: Reader {
    /// Bring the store up to date with where it is kept. For a ref this is a
    /// fetch, and a failed fetch is an error and never an empty store.
    fn refresh(&self) -> Result<Freshness, StoreError>;

    /// Write `edit` as one atomic step with `message`. When another writer
    /// moves the tip first, `hook` says what the edit becomes on the new tip.
    fn apply(
        &self,
        edit: &Edit,
        message: &str,
        hook: &dyn MergeHook,
    ) -> Result<Applied, StoreError>;
}
