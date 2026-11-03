//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A pairing over a ref store: the pairing of one of its commits, and the commit a pairing names.
//!
//! [`crate::pairing`] decides from candidates and a way to read their files. This is where the
//! candidates come from: the history of the store's local ref, with each commit's author time,
//! and the files of a commit read through the store.

use mockspace_store::RefStore;

use crate::pairing::{self, Candidate, Pairing, Resolution, TreeHasher};

/// The [`pairing::tree_digest`] of the files of `commit`.
///
/// # Errors
///
/// When the commit cannot be read, or is not an object id.
pub fn digest_of(store: &RefStore, commit: &str) -> Result<String, String> {
    let files = store.files_of(commit).map_err(|e| e.to_string())?;
    let mut hasher = TreeHasher::new();
    for (path, bytes) in &files {
        hasher.file(path, bytes)?;
    }
    Ok(hasher.finish())
}

/// The pairing of `commit` of the local ref, or of its tip for `None`: the hash, the author time
/// and the digest of the files.
///
/// # Errors
///
/// When the ref has no commits, `commit` is not on it, or git fails.
pub fn pairing_of(store: &RefStore, commit: Option<&str>) -> Result<Pairing, String> {
    let local = store.pair().local();
    let history = store.history().map_err(|e| e.to_string())?;
    let wanted = match commit {
        Some(commit) => commit.to_owned(),
        None => {
            store
                .local_tip()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("`{local}` has no commits"))?
        },
    };
    let found = history
        .iter()
        .find(|c| c.id == wanted)
        .ok_or_else(|| format!("`{wanted}` is not a commit of `{local}`"))?;
    Pairing::new(
        wanted.clone(),
        found.author_time,
        digest_of(store, &wanted)?,
    )
}

/// Which commit of the local ref `recorded` names, by [`pairing::resolve`] over its history.
///
/// A [`Resolution::Missing`] from a store whose history is cut short (a shallow fetch) is given
/// as [`Resolution::Truncated`], because the commit may only be one that was not fetched.
///
/// # Errors
///
/// When the history or the files of a commit cannot be read. A commit that cannot be read is
/// never skipped as though it did not match.
pub fn resolve(
    store: &RefStore,
    recorded: &Pairing,
    window_secs: i64,
) -> Result<Resolution, String> {
    let history = store.history().map_err(|e| e.to_string())?;
    let candidates: Vec<Candidate<'_>> = history
        .iter()
        .map(|c| {
            Candidate {
                hash: &c.id,
                time: c.author_time,
            }
        })
        .collect();
    let mut failure: Option<String> = None;
    let found = pairing::resolve(recorded, &candidates, window_secs, |hash| {
        digest_of(store, hash)
            .map_err(|e| failure.get_or_insert(e))
            .ok()
    });
    if let Some(failure) = failure {
        return Err(failure);
    }
    if found == Resolution::Missing && store.is_shallow().map_err(|e| e.to_string())? {
        return Ok(Resolution::Truncated);
    }
    Ok(found)
}

#[cfg(test)]
#[path = "pairing_ref_tests.rs"]
mod tests;
