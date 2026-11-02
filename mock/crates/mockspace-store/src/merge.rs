//! What happens to an edit when another writer's commit won the race.

use crate::edit::Edit;
use crate::error::StoreError;
use crate::reader::Reader;

/// The edit was built on a tip that is no longer the remote's. The hook sees the
/// tip that won, through `tip`, and the edit as its author wrote it, and returns
/// the edit to write on that tip. The store never merges file contents itself:
/// that is the namespace's rule, so it is the namespace's hook.
pub trait MergeHook {
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal>;
}

/// A hook's reason for not rebasing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRefusal(pub String);

impl From<MergeRefusal> for StoreError {
    fn from(r: MergeRefusal) -> Self {
        StoreError::Merge {
            reason: r.0,
        }
    }
}

/// Write the same edit again on the new tip. Right where every write adds paths
/// no other writer can have, as in a channel of uniquely named messages.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reapply;

impl MergeHook for Reapply {
    fn rebase(&self, _tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        Ok(ours.clone())
    }
}

/// Never rebase: a lost race is the caller's to handle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Refuse;

impl MergeHook for Refuse {
    fn rebase(&self, _tip: &dyn Reader, _ours: &Edit) -> Result<Edit, MergeRefusal> {
        Err(MergeRefusal(
            "another writer moved the tip and no merge rule was given".into(),
        ))
    }
}

impl<F> MergeHook for F
where
    F: Fn(&dyn Reader, &Edit) -> Result<Edit, MergeRefusal>,
{
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        self(tip, ours)
    }
}
