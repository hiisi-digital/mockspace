//! Where the data of one mockspace namespace lives, behind one contract.
//!
//! A [`Store`] is a directory ([`DirStore`]) or a git ref ([`RefStore`]). Both
//! read through [`Reader`] and take a write as an [`Edit`], one atomic step. The
//! ref backend writes a commit on top of the tip with `--force-with-lease`, so a
//! namespace keeps its history, and answers a lost race by fetching again and
//! asking a [`MergeHook`] what the edit is on the tip that won. Fetch and push
//! go by explicit refspec through a [`RefPair`], so a namespace kept on
//! `refs/mock/...` can be kept on `refs/heads/mock/...` instead by changing the
//! pair and nothing else.
//!
//! Plain git only: the crate runs the `git` command and asks no forge anything.
//!
//! What a store offers: read, list, snapshot, refresh, apply. What it leaves
//! out: merging file contents (the hook's, per namespace), validating what the
//! files hold (the writer's), deleting or rewriting history, signing, and any
//! knowledge of what a registry, a round or a test history is.

mod commit_reader;
mod dir;
mod edit;
mod error;
mod git;
mod identity;
mod merge;
mod path;
mod reader;
mod ref_store;
mod refs;
mod retry;
mod store;

pub use dir::DirStore;
pub use edit::Edit;
pub use error::StoreError;
pub use identity::Identity;
pub use merge::{MergeHook, MergeRefusal, Reapply, Refuse};
pub use path::validate_path;
pub use reader::Reader;
pub use ref_store::RefStore;
pub use refs::RefPair;
pub use retry::RetryPolicy;
pub use store::{Applied, Freshness, Store};
