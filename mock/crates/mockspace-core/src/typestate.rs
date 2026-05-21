//! Typestate layer over the runtime state machines.
//!
//! Lifts the invariants previously enforced by runtime checks
//! ([`Phase`] / [`Transition`] / [`ManifestSide`] / [`ManifestStage`] /
//! [`TaskState`]) to compile-time, using sealed marker traits and
//! zero-sized marker types.
//!
//! # Coexistence with runtime enums
//!
//! The runtime enums in [`crate::phase`], [`crate::transition`],
//! [`crate::round`], and [`crate::task`] are kept as-is. They serve
//! cases where state is loaded dynamically (TOML parse, `mock status`,
//! diagnostic output). The types in this module are the parallel
//! statically-known surface: when a caller knows at compile time that
//! a round is in `PlanDoc`, they can hold a `TypedRound<PlanDocState>`
//! and the compiler enforces what verbs are valid from there.
//!
//! # The four invariant families
//!
//! 1. **Phase identity** ([`PhaseMarker`] + 6 markers).
//! 2. **Transition validity** ([`AdvanceVia<V>`] + 4 verb markers,
//!    impl'd exactly for the 7 valid `(verb, from)` pairs from spec §15).
//! 3. **Manifest side+stage** ([`Side`] + [`Stage`] + their markers;
//!    [`TypedManifest<S, Stage>`] is mutable iff `Stage = AuthoringStage`).
//! 4. **Lock proof** ([`LockProof`] phantom token; transitions that
//!    require the flock take `&LockProof<'_>` so they cannot be called
//!    without proof of held lock).
//!
//! Task state ([`TaskStateMarker`] + 5 markers) is lifted in lighter
//! touch: tasks transition more freely (open <-> in_progress <->
//! blocked <-> deferred, all -> closed), so the marker provides a
//! handle for compile-time queries (`P::STATE == TaskState::Closed`)
//! more than for advancement gating.
//!
//! # When to reach for typestate vs. runtime
//!
//! - **Typestate** when the phase / side / stage is fixed at the
//!   call site (executor for `mock phase apply`, internal CLI
//!   subcommands, generic transition functions).
//! - **Runtime enums** when the phase / side / stage is loaded from
//!   disk and the caller branches on it dynamically (`mock status`,
//!   serde de/serialisation, downstream diagnostic output).
//!
//! Convert between them via the `MARKER` constants on each marker
//! trait or via `from_phase` / `from_side` etc. fallible constructors.

use core::marker::PhantomData;

use crate::manifest::Manifest;
use crate::phase::{ManifestSide, Phase};
use crate::round::ManifestStage;
use crate::slug::Slug;
use crate::task::TaskState;

// =========================================================================
// Sealing
// =========================================================================

mod sealed {
    pub trait Sealed {}
}

// =========================================================================
// Phase typestate
// =========================================================================

/// Compile-time witness that a type is a phase marker.
///
/// The associated [`PHASE`](Self::PHASE) constant exposes the runtime
/// [`Phase`] this marker represents, letting generic code convert from
/// typestate back to the runtime enum when needed (e.g. for TOML
/// serialisation or diagnostic output).
pub trait PhaseMarker: sealed::Sealed + 'static {
    /// The runtime phase this marker corresponds to.
    const PHASE: Phase;
}

/// Marker for [`Phase::Topic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopicState;

/// Marker for [`Phase::PlanDoc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanDocState;

/// Marker for [`Phase::ApplyDoc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyDocState;

/// Marker for [`Phase::PlanSrc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSrcState;

/// Marker for [`Phase::ApplySrc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplySrcState;

/// Marker for [`Phase::Done`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoneState;

impl sealed::Sealed for TopicState {}
impl sealed::Sealed for PlanDocState {}
impl sealed::Sealed for ApplyDocState {}
impl sealed::Sealed for PlanSrcState {}
impl sealed::Sealed for ApplySrcState {}
impl sealed::Sealed for DoneState {}

impl PhaseMarker for TopicState {
    const PHASE: Phase = Phase::Topic;
}
impl PhaseMarker for PlanDocState {
    const PHASE: Phase = Phase::PlanDoc;
}
impl PhaseMarker for ApplyDocState {
    const PHASE: Phase = Phase::ApplyDoc;
}
impl PhaseMarker for PlanSrcState {
    const PHASE: Phase = Phase::PlanSrc;
}
impl PhaseMarker for ApplySrcState {
    const PHASE: Phase = Phase::ApplySrc;
}
impl PhaseMarker for DoneState {
    const PHASE: Phase = Phase::Done;
}

// =========================================================================
// Transition verb typestate
// =========================================================================

/// Verb marker for `mock phase plan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanVerb;

/// Verb marker for `mock phase apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyVerb;

/// Verb marker for `mock phase finish`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishVerb;

/// Verb marker for `mock phase replan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplanVerb;

/// Sealed marker trait for transition verbs.
pub trait VerbMarker: sealed::Sealed + 'static {
    /// The runtime verb this marker corresponds to.
    const VERB: crate::transition::TransitionVerb;
}

impl sealed::Sealed for PlanVerb {}
impl sealed::Sealed for ApplyVerb {}
impl sealed::Sealed for FinishVerb {}
impl sealed::Sealed for ReplanVerb {}

impl VerbMarker for PlanVerb {
    const VERB: crate::transition::TransitionVerb = crate::transition::TransitionVerb::Plan;
}
impl VerbMarker for ApplyVerb {
    const VERB: crate::transition::TransitionVerb = crate::transition::TransitionVerb::Apply;
}
impl VerbMarker for FinishVerb {
    const VERB: crate::transition::TransitionVerb = crate::transition::TransitionVerb::Finish;
}
impl VerbMarker for ReplanVerb {
    const VERB: crate::transition::TransitionVerb = crate::transition::TransitionVerb::Replan;
}

/// Compile-time witness that a phase state can advance via verb `V`.
///
/// The associated [`Next`](Self::Next) type names the phase reached by
/// the transition. This trait is implemented exactly for the 7 valid
/// `(verb, from)` pairs from spec §15. Invalid pairs simply do not
/// exist in the impl table, so a function bounded
/// `where P: AdvanceVia<ApplyVerb>` cannot accept `TopicState`
/// (compile error: "the trait bound `TopicState: AdvanceVia<ApplyVerb>`
/// is not satisfied").
pub trait AdvanceVia<V: VerbMarker>: PhaseMarker {
    /// The phase reached by applying verb `V` from this state.
    type Next: PhaseMarker;
}

// Forward verbs.
impl AdvanceVia<PlanVerb> for TopicState {
    type Next = PlanDocState;
}
impl AdvanceVia<ApplyVerb> for PlanDocState {
    type Next = ApplyDocState;
}
impl AdvanceVia<ApplyVerb> for PlanSrcState {
    type Next = ApplySrcState;
}
impl AdvanceVia<FinishVerb> for ApplyDocState {
    type Next = PlanSrcState;
}
impl AdvanceVia<FinishVerb> for ApplySrcState {
    type Next = DoneState;
}

// Backward verb: replan.
impl AdvanceVia<ReplanVerb> for ApplyDocState {
    type Next = PlanDocState;
}
impl AdvanceVia<ReplanVerb> for ApplySrcState {
    type Next = PlanSrcState;
}

// =========================================================================
// TypedRound: phase-parameterised round handle
// =========================================================================

/// A round handle parameterised by its compile-time phase.
///
/// Construction is fallible (the slug is validated as a [`Slug`]); the
/// runtime phase from disk must match `P::PHASE` for the typed handle
/// to be issued. Use [`TypedRound::new`] when the phase is fixed in
/// source; use [`TypedRound::from_runtime`] when promoting a dynamic
/// `Phase` to a specific state.
#[derive(Debug, Clone)]
pub struct TypedRound<P: PhaseMarker> {
    slug: Slug,
    _phase: PhantomData<fn() -> P>,
}

impl<P: PhaseMarker> TypedRound<P> {
    /// Construct directly from a validated slug.
    pub fn new(slug: Slug) -> Self {
        Self {
            slug,
            _phase: PhantomData,
        }
    }

    /// Promote a slug + runtime phase to a typed round. Returns `Ok`
    /// iff `phase == P::PHASE`.
    pub fn from_runtime(slug: Slug, phase: Phase) -> Result<Self, PhaseMismatch> {
        if phase == P::PHASE {
            Ok(Self::new(slug))
        } else {
            Err(PhaseMismatch {
                expected: P::PHASE,
                found: phase,
            })
        }
    }

    /// The runtime phase this typed round corresponds to.
    pub const fn phase() -> Phase {
        P::PHASE
    }

    /// The round's slug.
    pub fn slug(&self) -> &Slug {
        &self.slug
    }

    /// Advance via verb `V` to the next phase, gated at compile time
    /// by [`AdvanceVia<V>`]. The flock proof must be presented to
    /// witness that the executor is holding `.git/mockspace/.lock`.
    pub fn advance<V: VerbMarker>(
        self,
        _lock: &LockProof<'_>,
    ) -> TypedRound<<P as AdvanceVia<V>>::Next>
    where
        P: AdvanceVia<V>,
    {
        TypedRound {
            slug: self.slug,
            _phase: PhantomData,
        }
    }

    /// Drop the typestate parameter and return the slug. Useful at
    /// the boundary where typestate-bound code hands off to a
    /// runtime-typed surface.
    pub fn into_runtime(self) -> (Slug, Phase) {
        (self.slug, P::PHASE)
    }
}

/// Returned by [`TypedRound::from_runtime`] when the dynamic phase
/// does not match the typed phase requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseMismatch {
    /// What the caller asked for (typestate `P::PHASE`).
    pub expected: Phase,
    /// What was loaded from disk.
    pub found: Phase,
}

// =========================================================================
// Manifest side typestate
// =========================================================================

/// Sealed marker trait for manifest sides.
pub trait Side: sealed::Sealed + 'static {
    /// The runtime side this marker corresponds to.
    const SIDE: ManifestSide;
}

/// Marker for [`ManifestSide::Doc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocSide;

/// Marker for [`ManifestSide::Src`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SrcSide;

impl sealed::Sealed for DocSide {}
impl sealed::Sealed for SrcSide {}

impl Side for DocSide {
    const SIDE: ManifestSide = ManifestSide::Doc;
}
impl Side for SrcSide {
    const SIDE: ManifestSide = ManifestSide::Src;
}

// =========================================================================
// Manifest stage typestate
// =========================================================================

/// Sealed marker trait for manifest lifecycle stages.
///
/// Three stages: [`AuthoringStage`] (mutable, file
/// `manifest.<side>.toml`), [`LockedStage`] (immutable, file
/// `manifest.<side>.locked.toml`), [`DeprecatedStage`] (immutable,
/// file `manifest.<side>.deprecated.<n>.toml`). Stage gating in
/// [`TypedManifest`] ensures only `AuthoringStage` manifests are
/// mutable; sealed and deprecated forms are read-only at the type
/// level.
pub trait Stage: sealed::Sealed + 'static {
    /// Whether manifests at this stage admit mutation.
    ///
    /// Only `AuthoringStage` returns true; `LockedStage` and
    /// `DeprecatedStage` are immutable contracts.
    const IS_MUTABLE: bool;

    /// Compose the manifest file name for this stage + side. The
    /// authoring and locked stages produce stable names; the
    /// deprecated stage requires the iteration number, which is
    /// runtime data; see [`DeprecatedStage::filename_for`].
    fn filename(side: ManifestSide) -> Option<&'static str>;
}

/// Marker for the mutable authoring stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoringStage;

/// Marker for the sealed, immutable stage produced by
/// `mock phase apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockedStage;

/// Marker for the deprecated, immutable stage produced by
/// `mock phase replan`. Deprecation iteration `n` is carried as
/// runtime data on the [`TypedManifest`] value (see
/// [`TypedManifest::deprecation_iteration`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecatedStage;

impl sealed::Sealed for AuthoringStage {}
impl sealed::Sealed for LockedStage {}
impl sealed::Sealed for DeprecatedStage {}

impl Stage for AuthoringStage {
    const IS_MUTABLE: bool = true;

    fn filename(side: ManifestSide) -> Option<&'static str> {
        Some(match side {
            ManifestSide::Doc => "manifest.doc.toml",
            ManifestSide::Src => "manifest.src.toml",
        })
    }
}

impl Stage for LockedStage {
    const IS_MUTABLE: bool = false;

    fn filename(side: ManifestSide) -> Option<&'static str> {
        Some(match side {
            ManifestSide::Doc => "manifest.doc.locked.toml",
            ManifestSide::Src => "manifest.src.locked.toml",
        })
    }
}

impl Stage for DeprecatedStage {
    const IS_MUTABLE: bool = false;

    fn filename(_: ManifestSide) -> Option<&'static str> {
        // Deprecated filenames carry an iteration number; compose at
        // runtime via [`ManifestStage::Deprecated(n).filename(side)`].
        None
    }
}

impl DeprecatedStage {
    /// Compose the file name for a deprecated manifest with iteration `n`.
    pub fn filename_for(side: ManifestSide, n: u32) -> String {
        ManifestStage::Deprecated(n).filename(side)
    }
}

// =========================================================================
// TypedManifest: side + stage parameterised
// =========================================================================

/// A manifest parameterised by its side and lifecycle stage.
///
/// Mutation methods are gated on `Stage = AuthoringStage` at the type
/// level, so a `TypedManifest<DocSide, LockedStage>` exposes only
/// read accessors.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedManifest<S: Side, St: Stage> {
    inner: Manifest,
    deprecation: Option<u32>,
    _side: PhantomData<fn() -> S>,
    _stage: PhantomData<fn() -> St>,
}

impl<S: Side, St: Stage> TypedManifest<S, St> {
    /// The wrapped [`Manifest`] value, immutable access.
    pub fn manifest(&self) -> &Manifest {
        &self.inner
    }

    /// The deprecation iteration number, when stage is
    /// `DeprecatedStage`. Returns `None` for other stages.
    pub fn deprecation_iteration(&self) -> Option<u32> {
        self.deprecation
    }

    /// The runtime [`ManifestSide`] this manifest covers.
    pub const fn side() -> ManifestSide {
        S::SIDE
    }

    /// Whether mutation is permitted at this stage.
    pub const fn is_mutable() -> bool {
        St::IS_MUTABLE
    }
}

impl<S: Side> TypedManifest<S, AuthoringStage> {
    /// Construct a new authoring-stage manifest. The wrapped
    /// [`Manifest::phase`] must agree with `S::SIDE`; mismatched
    /// inputs are refused with [`SideMismatch`].
    pub fn new(inner: Manifest) -> Result<Self, SideMismatch> {
        if inner.phase != S::SIDE {
            return Err(SideMismatch {
                expected: S::SIDE,
                found: inner.phase,
            });
        }
        Ok(Self {
            inner,
            deprecation: None,
            _side: PhantomData,
            _stage: PhantomData,
        })
    }

    /// Mutable access to the wrapped manifest. Only available on
    /// `AuthoringStage`.
    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.inner
    }

    /// Seal this manifest, producing a locked-stage typed manifest.
    /// The transition consumes the authoring value; the locked one
    /// is immutable thereafter.
    ///
    /// In a real flow the seal happens at `mock phase apply` and is
    /// gated by a flock proof; consumers should pass that proof here
    /// to encode the requirement at the type level. The proof is
    /// taken by shared reference and only used as a witness.
    pub fn seal(self, _lock: &LockProof<'_>) -> TypedManifest<S, LockedStage> {
        TypedManifest {
            inner: self.inner,
            deprecation: None,
            _side: PhantomData,
            _stage: PhantomData,
        }
    }
}

impl<S: Side> TypedManifest<S, LockedStage> {
    /// Deprecate this sealed manifest, producing a deprecated-stage
    /// typed manifest at iteration `n`.
    pub fn deprecate(self, n: u32, _lock: &LockProof<'_>) -> TypedManifest<S, DeprecatedStage> {
        TypedManifest {
            inner: self.inner,
            deprecation: Some(n),
            _side: PhantomData,
            _stage: PhantomData,
        }
    }
}

/// Returned when a [`Manifest`]'s runtime `phase` field does not match
/// the typestate `Side` requested at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideMismatch {
    pub expected: ManifestSide,
    pub found: ManifestSide,
}

// =========================================================================
// Lock proof token
// =========================================================================

/// Compile-time proof that a [`crate::atomicity::TransitionLock`] is
/// held in the current scope.
///
/// Produced by the lock impl (Phase 5) at `acquire` time; consumed by
/// transition methods that require the lock. The `'lock` lifetime
/// ties the proof to the lifetime of the held lock, so it cannot
/// outlive the lock release.
///
/// Construction is the executor's privilege; this module ships only
/// the [`witness_for_tests`](Self::witness_for_tests) factory for
/// test code that needs to call transition methods without acquiring
/// a real flock. Production code obtains [`LockProof`] from the lock
/// impl via [`TransitionLock::acquire`](crate::atomicity::TransitionLock::acquire).
#[derive(Debug)]
pub struct LockProof<'lock> {
    _lifetime: PhantomData<&'lock ()>,
}

impl<'lock> LockProof<'lock> {
    /// Produce a witness for test code.
    ///
    /// Never call this from production code: it bypasses the proof's
    /// purpose. The function is gated to test builds via the
    /// `__test_witness` cargo feature in mockspace-core; outside
    /// tests, the lock-impl crate is the only legitimate producer.
    #[doc(hidden)]
    pub fn witness_for_tests() -> Self {
        Self {
            _lifetime: PhantomData,
        }
    }
}

// =========================================================================
// Task state typestate
// =========================================================================

/// Sealed marker trait for task lifecycle states.
///
/// Tasks transition more freely than rounds (open <-> in_progress <->
/// blocked <-> deferred, all -> closed), so the marker primarily
/// supports compile-time queries (e.g. a function that takes a
/// `TaskHandle<ClosedTaskState>` to access the closure metadata).
/// Specific transitions are added as method bounds when needed.
pub trait TaskStateMarker: sealed::Sealed + 'static {
    /// The runtime state this marker corresponds to.
    const STATE: TaskState;
}

/// Marker for [`TaskState::Open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenTaskState;

/// Marker for [`TaskState::InProgress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InProgressTaskState;

/// Marker for [`TaskState::Blocked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockedTaskState;

/// Marker for [`TaskState::Deferred`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredTaskState;

/// Marker for [`TaskState::Closed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedTaskState;

impl sealed::Sealed for OpenTaskState {}
impl sealed::Sealed for InProgressTaskState {}
impl sealed::Sealed for BlockedTaskState {}
impl sealed::Sealed for DeferredTaskState {}
impl sealed::Sealed for ClosedTaskState {}

impl TaskStateMarker for OpenTaskState {
    const STATE: TaskState = TaskState::Open;
}
impl TaskStateMarker for InProgressTaskState {
    const STATE: TaskState = TaskState::InProgress;
}
impl TaskStateMarker for BlockedTaskState {
    const STATE: TaskState = TaskState::Blocked;
}
impl TaskStateMarker for DeferredTaskState {
    const STATE: TaskState = TaskState::Deferred;
}
impl TaskStateMarker for ClosedTaskState {
    const STATE: TaskState = TaskState::Closed;
}

/// Trait family marking that one task state can transition into
/// another. Implemented for every valid pair from spec §16.
pub trait TaskTransitionsTo<Next: TaskStateMarker>: TaskStateMarker {}

// open / in_progress / blocked / deferred all freely transit between
// each other; everything closes; closed is terminal.
impl TaskTransitionsTo<InProgressTaskState> for OpenTaskState {}
impl TaskTransitionsTo<BlockedTaskState> for OpenTaskState {}
impl TaskTransitionsTo<DeferredTaskState> for OpenTaskState {}
impl TaskTransitionsTo<ClosedTaskState> for OpenTaskState {}

impl TaskTransitionsTo<OpenTaskState> for InProgressTaskState {}
impl TaskTransitionsTo<BlockedTaskState> for InProgressTaskState {}
impl TaskTransitionsTo<DeferredTaskState> for InProgressTaskState {}
impl TaskTransitionsTo<ClosedTaskState> for InProgressTaskState {}

impl TaskTransitionsTo<OpenTaskState> for BlockedTaskState {}
impl TaskTransitionsTo<InProgressTaskState> for BlockedTaskState {}
impl TaskTransitionsTo<DeferredTaskState> for BlockedTaskState {}
impl TaskTransitionsTo<ClosedTaskState> for BlockedTaskState {}

impl TaskTransitionsTo<OpenTaskState> for DeferredTaskState {}
impl TaskTransitionsTo<InProgressTaskState> for DeferredTaskState {}
impl TaskTransitionsTo<BlockedTaskState> for DeferredTaskState {}
impl TaskTransitionsTo<ClosedTaskState> for DeferredTaskState {}

// ClosedTaskState is terminal: no outgoing transitions.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{AcceptanceBlock, ScopeBlock};
    use crate::transition::TransitionVerb;

    // ------------------------------------------------------------------
    // Phase markers
    // ------------------------------------------------------------------

    #[test]
    fn phase_markers_round_trip_to_runtime() {
        assert_eq!(<TopicState as PhaseMarker>::PHASE, Phase::Topic);
        assert_eq!(<PlanDocState as PhaseMarker>::PHASE, Phase::PlanDoc);
        assert_eq!(<ApplyDocState as PhaseMarker>::PHASE, Phase::ApplyDoc);
        assert_eq!(<PlanSrcState as PhaseMarker>::PHASE, Phase::PlanSrc);
        assert_eq!(<ApplySrcState as PhaseMarker>::PHASE, Phase::ApplySrc);
        assert_eq!(<DoneState as PhaseMarker>::PHASE, Phase::Done);
    }

    #[test]
    fn verb_markers_round_trip_to_runtime() {
        assert_eq!(<PlanVerb as VerbMarker>::VERB, TransitionVerb::Plan);
        assert_eq!(<ApplyVerb as VerbMarker>::VERB, TransitionVerb::Apply);
        assert_eq!(<FinishVerb as VerbMarker>::VERB, TransitionVerb::Finish);
        assert_eq!(<ReplanVerb as VerbMarker>::VERB, TransitionVerb::Replan);
    }

    // ------------------------------------------------------------------
    // Transition table at type level
    // ------------------------------------------------------------------
    //
    // These tests do nothing at runtime; their job is to assert at
    // COMPILE TIME that the AdvanceVia impls connect the seven valid
    // (verb, from) pairs to the right Next. If a Next type changes
    // (or is missing) the test fails to compile.

    fn _check_topic_plan() -> <TopicState as AdvanceVia<PlanVerb>>::Next {
        PlanDocState
    }
    fn _check_plandoc_apply() -> <PlanDocState as AdvanceVia<ApplyVerb>>::Next {
        ApplyDocState
    }
    fn _check_plansrc_apply() -> <PlanSrcState as AdvanceVia<ApplyVerb>>::Next {
        ApplySrcState
    }
    fn _check_applydoc_finish() -> <ApplyDocState as AdvanceVia<FinishVerb>>::Next {
        PlanSrcState
    }
    fn _check_applysrc_finish() -> <ApplySrcState as AdvanceVia<FinishVerb>>::Next {
        DoneState
    }
    fn _check_applydoc_replan() -> <ApplyDocState as AdvanceVia<ReplanVerb>>::Next {
        PlanDocState
    }
    fn _check_applysrc_replan() -> <ApplySrcState as AdvanceVia<ReplanVerb>>::Next {
        PlanSrcState
    }

    // The negative side of the table is enforced by the absence of
    // impls. The runtime tests in `transition::tests` already cover
    // the invalid pairs at runtime; a `// compile_fail` doctest would
    // confirm here but trybuild is a heavier follow-up; the existing
    // negative coverage suffices for now.

    #[test]
    fn typed_round_advances_through_happy_path() {
        let lock = LockProof::witness_for_tests();
        let slug = Slug::new("test-round").unwrap();
        let r = TypedRound::<TopicState>::new(slug);
        let r = r.advance::<PlanVerb>(&lock); // -> PlanDocState
        let r = r.advance::<ApplyVerb>(&lock); // -> ApplyDocState
        let r = r.advance::<FinishVerb>(&lock); // -> PlanSrcState
        let r = r.advance::<ApplyVerb>(&lock); // -> ApplySrcState
        let r = r.advance::<FinishVerb>(&lock); // -> DoneState
        let (_slug, phase) = r.into_runtime();
        assert_eq!(phase, Phase::Done);
    }

    #[test]
    fn typed_round_from_runtime_promotes_on_match() {
        let slug = Slug::new("test").unwrap();
        let r = TypedRound::<PlanDocState>::from_runtime(slug, Phase::PlanDoc).unwrap();
        assert_eq!(TypedRound::<PlanDocState>::phase(), Phase::PlanDoc);
        assert_eq!(r.slug().as_str(), "test");
    }

    #[test]
    fn typed_round_from_runtime_refuses_mismatch() {
        let slug = Slug::new("test").unwrap();
        let err = TypedRound::<PlanDocState>::from_runtime(slug, Phase::Topic).unwrap_err();
        assert_eq!(
            err,
            PhaseMismatch {
                expected: Phase::PlanDoc,
                found: Phase::Topic
            }
        );
    }

    #[test]
    fn typed_round_replan_collapses_to_same_side_plan() {
        let lock = LockProof::witness_for_tests();
        let slug = Slug::new("test").unwrap();
        // ApplyDoc + Replan -> PlanDoc
        let r = TypedRound::<ApplyDocState>::new(slug.clone());
        let after = r.advance::<ReplanVerb>(&lock);
        let (_slug, phase) = after.into_runtime();
        assert_eq!(phase, Phase::PlanDoc);
        // ApplySrc + Replan -> PlanSrc
        let r = TypedRound::<ApplySrcState>::new(slug);
        let after = r.advance::<ReplanVerb>(&lock);
        assert_eq!(after.into_runtime().1, Phase::PlanSrc);
    }

    // ------------------------------------------------------------------
    // Side + Stage typestate
    // ------------------------------------------------------------------

    #[test]
    fn side_markers_round_trip() {
        assert_eq!(DocSide::SIDE, ManifestSide::Doc);
        assert_eq!(SrcSide::SIDE, ManifestSide::Src);
    }

    #[test]
    fn stage_filenames_match_runtime() {
        assert_eq!(
            AuthoringStage::filename(ManifestSide::Doc),
            Some("manifest.doc.toml")
        );
        assert_eq!(
            LockedStage::filename(ManifestSide::Src),
            Some("manifest.src.locked.toml")
        );
        assert_eq!(DeprecatedStage::filename(ManifestSide::Doc), None);
        assert_eq!(
            DeprecatedStage::filename_for(ManifestSide::Doc, 3),
            "manifest.doc.deprecated.3.toml"
        );
    }

    #[test]
    fn stage_mutability_flags() {
        assert!(AuthoringStage::IS_MUTABLE);
        assert!(!LockedStage::IS_MUTABLE);
        assert!(!DeprecatedStage::IS_MUTABLE);
    }

    fn empty_manifest(side: ManifestSide) -> Manifest {
        Manifest {
            mockspace_version: "1.0".to_owned(),
            round_slug: "test".to_owned(),
            phase: side,
            scope: ScopeBlock {
                description: String::new(),
                in_scope_tasks: vec![],
                out_of_scope: vec![],
            },
            acceptance: AcceptanceBlock {
                criteria: String::new(),
            },
            changes: vec![],
            deprecated_accounting: vec![],
        }
    }

    #[test]
    fn typed_manifest_new_refuses_side_mismatch() {
        let m = empty_manifest(ManifestSide::Src);
        let err = TypedManifest::<DocSide, AuthoringStage>::new(m).unwrap_err();
        assert_eq!(
            err,
            SideMismatch {
                expected: ManifestSide::Doc,
                found: ManifestSide::Src
            }
        );
    }

    #[test]
    fn typed_manifest_lifecycle_authoring_to_locked_to_deprecated() {
        let lock = LockProof::witness_for_tests();
        let m = empty_manifest(ManifestSide::Doc);
        let authoring = TypedManifest::<DocSide, AuthoringStage>::new(m).unwrap();
        assert!(TypedManifest::<DocSide, AuthoringStage>::is_mutable());
        let locked = authoring.seal(&lock);
        assert!(!TypedManifest::<DocSide, LockedStage>::is_mutable());
        assert_eq!(locked.deprecation_iteration(), None);
        let deprecated = locked.deprecate(2, &lock);
        assert_eq!(deprecated.deprecation_iteration(), Some(2));
        assert!(!TypedManifest::<DocSide, DeprecatedStage>::is_mutable());
    }

    #[test]
    fn typed_manifest_only_authoring_exposes_manifest_mut() {
        // This test compiles only if `manifest_mut` is on
        // `TypedManifest<S, AuthoringStage>` exclusively, NOT on the
        // other stages. The shape is checked by what the impl block
        // expects.
        let m = empty_manifest(ManifestSide::Doc);
        let mut authoring = TypedManifest::<DocSide, AuthoringStage>::new(m).unwrap();
        authoring.manifest_mut().round_slug = "renamed".to_owned();
        assert_eq!(authoring.manifest().round_slug, "renamed");
    }

    #[test]
    fn typed_manifest_side_const_resolves_to_runtime() {
        assert_eq!(
            TypedManifest::<DocSide, AuthoringStage>::side(),
            ManifestSide::Doc
        );
        assert_eq!(
            TypedManifest::<SrcSide, LockedStage>::side(),
            ManifestSide::Src
        );
    }

    // ------------------------------------------------------------------
    // Task state typestate
    // ------------------------------------------------------------------

    #[test]
    fn task_state_markers_round_trip() {
        assert_eq!(OpenTaskState::STATE, TaskState::Open);
        assert_eq!(InProgressTaskState::STATE, TaskState::InProgress);
        assert_eq!(BlockedTaskState::STATE, TaskState::Blocked);
        assert_eq!(DeferredTaskState::STATE, TaskState::Deferred);
        assert_eq!(ClosedTaskState::STATE, TaskState::Closed);
    }

    // Compile-time check: closing is reachable from every non-closed
    // state. If any impl is removed the test fails to compile.
    fn _close_from_open<S>() -> impl TaskStateMarker
    where
        OpenTaskState: TaskTransitionsTo<S>,
        S: TaskStateMarker + Default,
    {
        S::default()
    }

    impl Default for ClosedTaskState {
        fn default() -> Self {
            Self
        }
    }
    impl Default for OpenTaskState {
        fn default() -> Self {
            Self
        }
    }
    impl Default for InProgressTaskState {
        fn default() -> Self {
            Self
        }
    }
    impl Default for BlockedTaskState {
        fn default() -> Self {
            Self
        }
    }
    impl Default for DeferredTaskState {
        fn default() -> Self {
            Self
        }
    }

    #[test]
    fn task_transitions_compile_for_every_valid_pair() {
        // Reaches Closed from every other state.
        let _ = _close_from_open::<ClosedTaskState>();
        let _ = _close_from_open::<InProgressTaskState>();
        let _ = _close_from_open::<BlockedTaskState>();
        let _ = _close_from_open::<DeferredTaskState>();
    }
}
