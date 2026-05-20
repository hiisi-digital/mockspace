//! Phase transition verbs + validity matrix (spec §14, §15).
//!
//! Three forward verbs and one backward verb move a round between the six
//! phases:
//!
//! - [`Transition::Plan`] scaffolds the next manifest (from TOPIC).
//! - [`Transition::Apply`] seals the current manifest, runs the verifier,
//!   captures the anchor, pushes the new round tip (from PLAN(DOC) / PLAN(SRC)).
//! - [`Transition::Finish`] advances bookkeeping to the next PLAN or to DONE
//!   (from APPLY(DOC) / APPLY(SRC)).
//! - [`Transition::Replan`] deprecates the current sealed manifest and
//!   restores phase-owned surfaces from the anchor (from APPLY(DOC) / APPLY(SRC)).
//!
//! Phase 1 ships the value shapes + the validity matrix as pure data. The
//! IO machinery that performs each transition (flock, git update-ref, push,
//! anchor capture, manifest rename, render) lives in Phase 5 alongside the
//! CLI binding.
//!
//! See also [`crate::phase::Phase`] for the phase enum itself and
//! [`crate::atomicity`] for the atomicity contract the executor must satisfy.

use std::path::PathBuf;

use crate::phase::Phase;

/// The four phase-transition verbs.
///
/// Each variant carries any per-verb input data the executor needs. Most
/// transitions take no extra input; [`Replan`] is the exception (it carries
/// a [`ReplanMode`] capturing how restoration handles post-APPLY work).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// Open a planning surface; scaffold the next manifest.
    Plan,
    /// Seal the current manifest and transition to APPLY.
    Apply,
    /// Advance bookkeeping to the next PLAN or to DONE.
    Finish,
    /// Deprecate the current sealed manifest and return to PLAN.
    Replan(ReplanMode),
}

/// How `Transition::Replan` handles post-APPLY work that touches claimed files.
///
/// Default replan overwrites source-side files at restoration time. If
/// post-APPLY commits built on those files, [`Destructive`] mode refuses;
/// the caller picks an explicit recovery strategy via the other variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplanMode {
    /// Overwrite source-side files at restoration time. Refuses (the
    /// executor returns a hard error) if any post-APPLY commits touch
    /// claimed files. Default mode.
    Destructive,
    /// Commit the restoration on top of post-APPLY state rather than
    /// overwriting. History is cluttered (post-APPLY work + additive
    /// replan commit) but no work is lost.
    AdditiveByCommit,
    /// Accept post-APPLY work loss for the named claimed files. Other
    /// claimed files refuse as in `Destructive`.
    AcceptRestorationLoss(Vec<PathBuf>),
}

impl Default for ReplanMode {
    fn default() -> Self {
        Self::Destructive
    }
}

/// Whether a transition is valid from a given phase, and if so what phase
/// it lands the round in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionValidity {
    /// The transition is valid; the round will land in `next`.
    Valid {
        next: Phase,
    },
    /// The transition is invalid from the current phase.
    InvalidFromPhase {
        current: Phase,
        verb: TransitionVerb,
        allowed_from: &'static [Phase],
    },
}

/// The four transition verbs as a tagless enum, for error reporting where
/// the per-verb data is not relevant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionVerb {
    Plan,
    Apply,
    Finish,
    Replan,
}

impl TransitionVerb {
    /// The set of phases this verb is valid from, per spec §15.
    pub const fn allowed_from(self) -> &'static [Phase] {
        match self {
            // Spec §15: "Valid from TOPIC". (APPLY(DOC) → PLAN(SRC) and
            // APPLY(SRC) → DONE are bookkeeping advances handled by
            // `finish`, which also scaffolds the next manifest where
            // applicable.)
            Self::Plan => &[Phase::Topic],
            Self::Apply => &[Phase::PlanDoc, Phase::PlanSrc],
            Self::Finish => &[Phase::ApplyDoc, Phase::ApplySrc],
            Self::Replan => &[Phase::ApplyDoc, Phase::ApplySrc],
        }
    }

    /// The phase reached by applying this verb from `from`, when valid.
    ///
    /// Returns `None` when the verb is not valid from `from`.
    pub const fn next_phase(self, from: Phase) -> Option<Phase> {
        match (self, from) {
            (Self::Plan, Phase::Topic) => Some(Phase::PlanDoc),
            (Self::Apply, Phase::PlanDoc) => Some(Phase::ApplyDoc),
            (Self::Apply, Phase::PlanSrc) => Some(Phase::ApplySrc),
            (Self::Finish, Phase::ApplyDoc) => Some(Phase::PlanSrc),
            (Self::Finish, Phase::ApplySrc) => Some(Phase::Done),
            (Self::Replan, Phase::ApplyDoc) => Some(Phase::PlanDoc),
            (Self::Replan, Phase::ApplySrc) => Some(Phase::PlanSrc),
            _ => None,
        }
    }
}

impl Transition {
    /// The verb tag for this transition.
    pub const fn verb(&self) -> TransitionVerb {
        match self {
            Self::Plan => TransitionVerb::Plan,
            Self::Apply => TransitionVerb::Apply,
            Self::Finish => TransitionVerb::Finish,
            Self::Replan(_) => TransitionVerb::Replan,
        }
    }

    /// Validate this transition against the current phase.
    pub fn validate(&self, current: Phase) -> TransitionValidity {
        let verb = self.verb();
        match verb.next_phase(current) {
            Some(next) => TransitionValidity::Valid {
                next,
            },
            None => TransitionValidity::InvalidFromPhase {
                current,
                verb,
                allowed_from: verb.allowed_from(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(verb: TransitionVerb, from: Phase) -> Option<Phase> {
        verb.next_phase(from)
    }

    #[test]
    fn happy_path_plan_apply_finish_doc_side() {
        // Topic → plan → PlanDoc → apply → ApplyDoc → finish → PlanSrc
        assert_eq!(next(TransitionVerb::Plan, Phase::Topic), Some(Phase::PlanDoc));
        assert_eq!(
            next(TransitionVerb::Apply, Phase::PlanDoc),
            Some(Phase::ApplyDoc)
        );
        assert_eq!(
            next(TransitionVerb::Finish, Phase::ApplyDoc),
            Some(Phase::PlanSrc)
        );
    }

    #[test]
    fn happy_path_src_side() {
        // PlanSrc → apply → ApplySrc → finish → Done
        assert_eq!(
            next(TransitionVerb::Apply, Phase::PlanSrc),
            Some(Phase::ApplySrc)
        );
        assert_eq!(
            next(TransitionVerb::Finish, Phase::ApplySrc),
            Some(Phase::Done)
        );
    }

    #[test]
    fn replan_returns_to_plan_on_same_side() {
        assert_eq!(
            next(TransitionVerb::Replan, Phase::ApplyDoc),
            Some(Phase::PlanDoc)
        );
        assert_eq!(
            next(TransitionVerb::Replan, Phase::ApplySrc),
            Some(Phase::PlanSrc)
        );
    }

    #[test]
    fn replan_not_valid_from_plan_or_topic_or_done() {
        for from in [Phase::Topic, Phase::PlanDoc, Phase::PlanSrc, Phase::Done] {
            assert_eq!(next(TransitionVerb::Replan, from), None, "from {from:?}");
        }
    }

    #[test]
    fn done_is_terminal() {
        for verb in [
            TransitionVerb::Plan,
            TransitionVerb::Apply,
            TransitionVerb::Finish,
            TransitionVerb::Replan,
        ] {
            assert_eq!(next(verb, Phase::Done), None, "verb {verb:?}");
        }
    }

    #[test]
    fn plan_only_valid_from_topic() {
        for from in [
            Phase::PlanDoc,
            Phase::ApplyDoc,
            Phase::PlanSrc,
            Phase::ApplySrc,
            Phase::Done,
        ] {
            assert_eq!(next(TransitionVerb::Plan, from), None, "from {from:?}");
        }
    }

    #[test]
    fn apply_only_valid_from_plan() {
        for from in [Phase::Topic, Phase::ApplyDoc, Phase::ApplySrc, Phase::Done] {
            assert_eq!(next(TransitionVerb::Apply, from), None, "from {from:?}");
        }
    }

    #[test]
    fn finish_only_valid_from_apply() {
        for from in [Phase::Topic, Phase::PlanDoc, Phase::PlanSrc, Phase::Done] {
            assert_eq!(next(TransitionVerb::Finish, from), None, "from {from:?}");
        }
    }

    #[test]
    fn transition_validate_reports_invalid_with_allowed_set() {
        let result = Transition::Apply.validate(Phase::Topic);
        match result {
            TransitionValidity::InvalidFromPhase {
                current,
                verb,
                allowed_from,
            } => {
                assert_eq!(current, Phase::Topic);
                assert_eq!(verb, TransitionVerb::Apply);
                assert_eq!(allowed_from, &[Phase::PlanDoc, Phase::PlanSrc]);
            }
            other => panic!("expected invalid, got {other:?}"),
        }
    }

    #[test]
    fn transition_validate_reports_valid_next_phase() {
        match Transition::Plan.validate(Phase::Topic) {
            TransitionValidity::Valid { next } => assert_eq!(next, Phase::PlanDoc),
            other => panic!("expected valid, got {other:?}"),
        }
    }

    #[test]
    fn replan_carries_mode_through_verb_tag() {
        let t = Transition::Replan(ReplanMode::AcceptRestorationLoss(vec![
            PathBuf::from("crates/x/src/lib.rs"),
        ]));
        assert_eq!(t.verb(), TransitionVerb::Replan);
        assert!(matches!(
            t.validate(Phase::ApplyDoc),
            TransitionValidity::Valid {
                next: Phase::PlanDoc
            }
        ));
    }

    #[test]
    fn replan_mode_default_is_destructive() {
        assert_eq!(ReplanMode::default(), ReplanMode::Destructive);
    }

    #[test]
    fn verb_allowed_from_matches_next_phase_table() {
        // For every verb, every phase in `allowed_from` must yield Some
        // from `next_phase`, and every phase NOT in `allowed_from` must
        // yield None. This locks the table against drift.
        for verb in [
            TransitionVerb::Plan,
            TransitionVerb::Apply,
            TransitionVerb::Finish,
            TransitionVerb::Replan,
        ] {
            let allowed = verb.allowed_from();
            for from in [
                Phase::Topic,
                Phase::PlanDoc,
                Phase::ApplyDoc,
                Phase::PlanSrc,
                Phase::ApplySrc,
                Phase::Done,
            ] {
                let got = verb.next_phase(from);
                let expected = allowed.contains(&from);
                assert_eq!(
                    got.is_some(),
                    expected,
                    "verb {verb:?} from {from:?}: next={got:?}, in allowed_from={expected}"
                );
            }
        }
    }
}
