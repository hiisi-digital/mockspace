//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The shared store the generated lint crates live in.
//!
//! One directory per key under a base outside every worktree. The key hashes
//! what makes two builds differ (the repo's identity, the mock directory below
//! the repo root, the engine's lint-rules dependency and the resolved pack
//! revisions) and nothing about where a worktree sits, so the worktrees of one
//! repo at the same revisions are one cargo package and share its dependency
//! builds.
//!
//! A key directory is one crate, written by whichever worktree runs, so every
//! run takes the directory's lock for the write, the build and the load. The
//! worktree that wrote it last is recorded as its owner, and `mock test` uses
//! that to test only a crate that is still its own.
//!
//! A new key appears whenever a pack tip or the lint-rules revision moves, and
//! the old one is never used again, so [`reap`] removes the key directories
//! nobody can come back to.

use std::time::Duration;

mod key;
mod lock;
mod record;
mod reap;

#[cfg(test)]
pub(crate) mod knobs;
#[cfg(test)]
mod support;

pub(crate) use key::{absolutize_path_pack, lints_manifest_path, resolve_packs, shared_base};
pub(crate) use lock::lock_dir;
pub(crate) use reap::reap;
pub(crate) use record::{
    Generated,
    ScopedPlan,
    duplicates_of,
    generated_dir_of,
    plan_scoped,
    record_duplicates,
    record_owner,
    touch_used,
};

const LOCK: &str = ".lock";
const OWNER: &str = ".owner";
const USED: &str = ".used";
const POINTER: &str = "shared-dir";

/// How long a key directory may go unused before [`reap`] removes it.
///
/// A key moves on every pack push and every lint-rules revision, and each
/// holds about 85 MB of build that nothing refers to once the key has moved, so
/// the number has to be short enough for those to clear within a sprint. It
/// also has to outlast a branch left alone for a while, whose first run after
/// the gap would otherwise pay a cold release build of the whole dependency
/// graph. Fourteen days sits between the two: a branch untouched for two weeks
/// rebuilds once, and a key abandoned by a moved pin is gone in two weeks. A
/// removal is only ever a rebuild, never a wrong answer, so the figure is a
/// disk-for-time trade and not a correctness bound.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(14 * 24 * 60 * 60);

#[cfg(test)]
#[path = "store_e2e_tests.rs"]
mod e2e;
