//! Phase 5 IO executors against the v2 orphan-ref storage substrate.
//!
//! The v2 design commits to git refs (orphan, flat per
//! `refs/mock/round/<slug>`) per spec §19, §24, §25. This module
//! ships the executors that read, write, and advance those refs.
//!
//! The slice plan lives at
//! `mock/research/202605220843_phase-5-io-slice-plan.md`. Slice E1
//! lands here: `RepoHandle::open` wrapping `gix::Repository`. Later
//! slices add ref readers/writers, the flock-based `TransitionLock`
//! impl, anchor capture, and the executors that compose them
//! (`seal_manifest`, `advance_phase`, `archive_round`).

mod advance;
mod anchor_capture;
mod archive;
mod lock;
mod ref_tree;
mod ref_write;
mod repo;
mod seal;
mod task;
mod time;

pub use advance::{AdvanceError, AdvanceReport, AdvanceVerb};
pub use anchor_capture::AnchorCaptureError;
pub use archive::{ArchiveError, ArchiveReport};
pub use lock::{FlockTransitionLock, LockError};
pub use ref_tree::{RefTreeReadError, RoundRefTree};
pub use ref_write::RefTreeWriteError;
pub use repo::{RepoError, RepoHandle};
pub use seal::{SealError, SealReport};
pub use task::{CreateTaskError, CreateTaskReport, ListTasksError, ShowTaskError};
