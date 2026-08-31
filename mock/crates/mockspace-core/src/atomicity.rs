//! Transition atomicity contract (spec §24).
//!
//! Phase 1 ships the contract surface only: trait signatures, value types,
//! race-conflict shapes, profile-driven race-action enums. The actual
//! flock(2) + git update-ref + push CAS + side-branch preservation
//! machinery lives in Phase 5.
//!
//! # Atomicity model
//!
//! A phase transition is a multi-step operation guarded by
//! `.git/mockspace/.lock` (BSD `flock(2)`). The lock is held for the
//! duration of the transition; auto-released on process exit. The lock
//! prevents in-process concurrent transitions on the same repo, but does
//! not prevent multi-machine races (two clones with different developers).
//! Those are caught by the push CAS at step 12 of the apply sequence and
//! routed through [`OnPhaseRaceAction`].
//!
//! The contract is "local-first commit, public-last announce": durable
//! state (manifest seal, anchor capture, source-side branch tip) lands
//! before any forge announcement (PR open, comment ingest).
//!
//! See spec §24 for the full sequence; this module just names the shapes
//! that sequence operates on.

use serde::{Deserialize, Serialize};

/// RAII guard over `.git/mockspace/.lock`.
///
/// Implementations of [`TransitionLock`] acquire the flock in
/// [`acquire`](TransitionLock::acquire) and release on Drop. Phase 5
/// supplies the concrete implementation; this trait names only the shape.
///
/// The lock file content is hostname + PID + start time for debugging.
/// The kernel manages the actual exclusion; userspace just writes the
/// debug payload.
pub trait TransitionLock: Sized {
    /// The error type produced by [`acquire`](Self::acquire).
    type Error: core::fmt::Debug + core::fmt::Display;

    /// Acquire the lock, blocking until available. Returns the guard.
    ///
    /// On filesystems that do not honour POSIX advisory locks (NFS,
    /// sshfs, cloud-sync paths), `acquire` may succeed without providing
    /// actual exclusion. `mock doctor` raises `D038` to surface the
    /// risk. See spec §24 "flock semantics and filesystem caveats".
    fn acquire() -> Result<Self, Self::Error>;

    /// A debug payload describing who holds the lock (hostname + PID +
    /// start time). Captured at acquire time; never re-read from the
    /// lock file.
    fn holder(&self) -> &LockHolder;
}

/// Debug payload identifying the holder of `.git/mockspace/.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockHolder {
    /// Hostname of the machine holding the lock.
    pub hostname:    String,
    /// OS process ID of the holder.
    pub pid:         u32,
    /// ISO-8601 timestamp at which the lock was acquired.
    pub acquired_at: String,
}

/// A phase-race conflict detected at push CAS time.
///
/// Spec §24 step 12: when `git push origin refs/mock/round/<slug>` returns
/// non-fast-forward, the local round-ref tip is preserved as a side branch
/// at `refs/mock/round/<slug>-conflict-<host>-<ts>` BEFORE any local reset.
/// The side-branch push must succeed on the remote before reset proceeds;
/// failure here is `D037` ("race conflict could not be preserved on
/// remote; local state retained").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseRaceConflict {
    /// Round whose ref produced the conflict.
    pub round_slug: String,
    /// Local tip SHA at conflict detection (the work being preserved).
    pub local_tip:  String,
    /// Remote tip SHA at the failed push (the work that won the race).
    pub remote_tip: String,
    /// Hostname of the machine that lost the race (for side-branch naming).
    pub host:       String,
    /// ISO-8601 timestamp at which the conflict was detected
    /// (for side-branch naming).
    pub timestamp:  String,
}

impl PhaseRaceConflict {
    /// The side-branch ref name where the local tip is preserved before
    /// any reset. Encodes hostname + timestamp to keep multiple races on
    /// the same round distinct.
    pub fn side_branch_ref(&self) -> String {
        format!(
            "refs/mock/round/{}-conflict-{}-{}",
            self.round_slug, self.host, self.timestamp
        )
    }
}

/// Action a profile dictates when a phase race is detected.
///
/// Set per [profile.<name>] in `mockspace.toml`'s `on_phase_race` key.
/// Default is [`Refuse`](Self::Refuse): the developer must run
/// `mock phase resolve <slug>` to inspect the conflict and pick a
/// resolution. Auto-resolve variants are opt-in for trusted automation
/// contexts only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnPhaseRaceAction {
    /// Refuse to advance; preserve the local work on the side branch and
    /// hand control back to the developer. Default.
    Refuse,
    /// Auto-resolve by keeping the local tip (push --force-with-lease
    /// onto the remote tip). Only safe in single-developer automation.
    AutoKeepLocal,
    /// Auto-resolve by discarding the local tip in favour of the remote.
    /// Local work is still preserved on the side branch for later
    /// recovery via `mock phase resolve`.
    AutoKeepRemote,
}

impl Default for OnPhaseRaceAction {
    fn default() -> Self {
        Self::Refuse
    }
}

/// Manual resolution choice the developer makes when running
/// `mock phase resolve <slug>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveStrategy {
    /// Keep the local tip; remote loses.
    KeepLocal,
    /// Keep the remote tip; local moves to the side branch (already
    /// done at conflict time).
    KeepRemote,
    /// Manual resolution outside mockspace (rebase, cherry-pick, etc).
    /// Mockspace logs the choice and steps out.
    Manual,
}

/// Diagnostic finding ID raised when an atomicity invariant fails.
///
/// Listed in spec §55. These IDs identify specific failure modes the
/// executor must surface verbatim so consumers can match against them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomicityFinding {
    /// `D008`: stale `.git/mockspace/.lock` (flock owner not running).
    StaleLock,
    /// `D012`: round ref local tip ahead of remote (recoverable).
    LocalAheadOfRemote,
    /// `D013`: round ref local tip behind remote (recoverable).
    LocalBehindRemote,
    /// `D020`: round-ref race; side-branch at `<conflict-ref>`.
    RaceConflict,
    /// `D037`: race conflict could not be preserved on remote;
    /// local state retained. Hard-stop; user intervention required.
    RaceConflictNotPreserved,
    /// `D038`: `.mock/` parent appears to be a cloud-sync directory.
    CloudSyncDetected,
}

impl AtomicityFinding {
    /// The `Dxxx` identifier from spec §55.
    pub const fn id(self) -> &'static str {
        match self {
            Self::StaleLock => "D008",
            Self::LocalAheadOfRemote => "D012",
            Self::LocalBehindRemote => "D013",
            Self::RaceConflict => "D020",
            Self::RaceConflictNotPreserved => "D037",
            Self::CloudSyncDetected => "D038",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_branch_ref_format() {
        let conflict = PhaseRaceConflict {
            round_slug: "202605181400-arvo-graph-csr".to_owned(),
            local_tip:  "abc123".to_owned(),
            remote_tip: "def456".to_owned(),
            host:       "alpha".to_owned(),
            timestamp:  "20260519T180000Z".to_owned(),
        };
        assert_eq!(
            conflict.side_branch_ref(),
            "refs/mock/round/202605181400-arvo-graph-csr-conflict-alpha-20260519T180000Z"
        );
    }

    #[test]
    fn on_phase_race_default_refuses_merge_automatically() {
        // The default action on a phase race is to refuse rather than
        // auto-keep either local or remote. Asserting against both
        // alternatives separately catches the silent-flip regression
        // where the default rotates to `AutoKeepLocal` or
        // `AutoKeepRemote` during a refactor.
        let d = OnPhaseRaceAction::default();
        assert_eq!(d, OnPhaseRaceAction::Refuse);
        assert_ne!(d, OnPhaseRaceAction::AutoKeepLocal);
        assert_ne!(d, OnPhaseRaceAction::AutoKeepRemote);
    }

    #[test]
    fn on_phase_race_serde_uses_snake_case() {
        #[derive(Serialize)]
        struct Wrap {
            action: OnPhaseRaceAction,
        }
        let s = toml::to_string(&Wrap {
            action: OnPhaseRaceAction::AutoKeepLocal,
        })
        .unwrap();
        assert!(
            s.contains("auto_keep_local"),
            "expected snake_case in serialised form: {s}"
        );
    }

    #[test]
    fn lock_holder_round_trips() {
        let holder = LockHolder {
            hostname:    "alpha".to_owned(),
            pid:         4242,
            acquired_at: "2026-05-19T18:00:00Z".to_owned(),
        };
        let s = toml::to_string(&holder).unwrap();
        let r: LockHolder = toml::from_str(&s).unwrap();
        assert_eq!(r, holder);
    }

    #[test]
    fn atomicity_finding_ids_match_spec() {
        assert_eq!(AtomicityFinding::StaleLock.id(), "D008");
        assert_eq!(AtomicityFinding::LocalAheadOfRemote.id(), "D012");
        assert_eq!(AtomicityFinding::LocalBehindRemote.id(), "D013");
        assert_eq!(AtomicityFinding::RaceConflict.id(), "D020");
        assert_eq!(AtomicityFinding::RaceConflictNotPreserved.id(), "D037");
        assert_eq!(AtomicityFinding::CloudSyncDetected.id(), "D038");
    }

    #[test]
    fn resolve_strategy_serialises_snake_case() {
        #[derive(Serialize)]
        struct Wrap {
            strategy: ResolveStrategy,
        }
        let s = toml::to_string(&Wrap {
            strategy: ResolveStrategy::KeepRemote,
        })
        .unwrap();
        assert!(s.contains("keep_remote"), "expected snake_case: {s}");
    }

    #[test]
    fn phase_race_conflict_round_trips() {
        let conflict = PhaseRaceConflict {
            round_slug: "abc".to_owned(),
            local_tip:  "11".to_owned(),
            remote_tip: "22".to_owned(),
            host:       "h".to_owned(),
            timestamp:  "20260519T180000Z".to_owned(),
        };
        let s = toml::to_string(&conflict).unwrap();
        let r: PhaseRaceConflict = toml::from_str(&s).unwrap();
        assert_eq!(r, conflict);
    }
}
