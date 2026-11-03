//! The hook an apply hands the store, wrapped so that the instance learns which files of its
//! edit the hook returned an entry for.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::edit::Edit;
use crate::merge::{MergeHook, MergeRefusal};
use crate::reader::Reader;

pub(super) struct Tracking<'h> {
    inner: &'h dyn MergeHook,
    kept:  Mutex<Option<BTreeSet<String>>>,
}

impl<'h> Tracking<'h> {
    pub(super) fn new(inner: &'h dyn MergeHook) -> Self {
        Self {
            inner,
            kept: Mutex::new(None),
        }
    }

    /// The paths of the edit the hook returned the last time it was asked, or `None` when it was
    /// never asked.
    pub(super) fn kept(&self) -> Option<BTreeSet<String>> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl MergeHook for Tracking<'_> {
    fn rebase(&self, tip: &dyn Reader, ours: &Edit) -> Result<Edit, MergeRefusal> {
        let next = self.inner.rebase(tip, ours)?;
        *self.kept.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(next.entries().map(|(path, _)| path.to_owned()).collect());
        Ok(next)
    }
}
