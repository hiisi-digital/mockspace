//! Reading: the part of a store that a merge hook is also handed.

use std::collections::BTreeMap;

use crate::error::StoreError;

/// Read access to one state of a store.
pub trait Reader {
    /// The bytes at `path`, or `None` when there is no file there.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError>;

    /// Every file path at or under `prefix` (the empty prefix is all of them),
    /// sorted. A prefix names whole components: `a` matches `a/b`, never `ab`. A directory store
    /// also lists an entry that is not a directory but cannot be read, a symlink to nothing, and
    /// `read` gives `None` for it; `snapshot` leaves it out.
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError>;

    /// Every file and its bytes, in path order.
    fn snapshot(&self) -> Result<BTreeMap<String, Vec<u8>>, StoreError> {
        let mut out = BTreeMap::new();
        for path in self.list("")? {
            if let Some(bytes) = self.read(&path)? {
                out.insert(path, bytes);
            }
        }
        Ok(out)
    }
}
