//! Where the data of one mockspace namespace lives, behind one contract.
//!
//! A [`Store`] is a directory ([`DirStore`]) or a git ref ([`RefStore`]). Both
//! read through [`Reader`] and take a write as an [`Edit`], one step. The
//! ref backend writes a commit on top of the tip with `--force-with-lease`, so a
//! namespace keeps its history, and answers a lost race by fetching again and
//! asking a [`MergeHook`] what the edit is on the tip that won. Fetch and push
//! go by explicit refspec through a [`RefPair`], so a namespace kept on
//! `refs/mock/...` can be kept on `refs/heads/mock/...` instead by changing the
//! pair and nothing else.
//!
//! Plain git only: the crate runs the `git` command and asks no forge anything.
//!
//! Scope against the v2 spec: this store writes history, which suits a namespace that keeps
//! its past (registry rows, tickets, test history; spec line 1513 and 1521 make bench results orphan, which this crate does not write). The spec makes the round
//! and task refs orphan (`mockspace-v2-spec.md` line 1521), and this crate does not write
//! those. Nor does it preserve a losing commit on a `-conflict-<host>-<ts>` side branch
//! before a reset (spec section 24, step 12c); a [`Refuse`] drops it. Both are owed by
//! whoever ports rounds, and neither is decided here.
//!
//! What a store offers: read, list, snapshot, refresh, apply. What it leaves
//! out: merging file contents (the hook's, per namespace), validating what the
//! files hold (the writer's), deleting or rewriting history, signing, and any
//! knowledge of what a registry, a round or a test history is.

pub mod attributes;
mod commit_reader;
mod dir;
mod edit;
mod error;
mod git;
mod identity;
mod ids;
mod instance;
mod merge;
mod path;
mod reader;
mod ref_history;
mod ref_ops;
mod ref_store;
mod refs;
mod retry;
mod store;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use dir::DirStore;
pub use edit::Edit;
pub use error::StoreError;
pub use git::{Git, Out};
pub use identity::Identity;
pub use ids::{Base, Oid};
pub use instance::{Held, Instance, Look, exclude_lines, sparse_lines};
pub use merge::{MergeHook, MergeRefusal, Reapply, Refuse};
pub use path::{symlinks_on, validate_path};
pub use reader::Reader;
pub use ref_history::CommitInfo;
pub use ref_store::{DATA_REF_ENV, RefStore};
pub use refs::{RefPair, RemoteForm};
pub use retry::RetryPolicy;
pub use store::{Applied, Freshness, Store};

/// What these tests take out of the environment before they run.
#[cfg(test)]
#[path = "../../../../tests/common/ambient.rs"]
mod test_ambient;
