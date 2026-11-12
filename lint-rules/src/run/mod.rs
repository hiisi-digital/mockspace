//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The runner: every lint of a run over one [`Reading`], each unit of work on a thread, each unit
//! replayed from its saved trace where that still holds.
//!
//! A unit is one lint over one thing: a file of a crate for a lint judging one file at a time, a
//! crate for one judging the crate whole, the bound crates for a workspace lint, the repository for
//! a repo lint. Every unit is first looked up in the store, on the run's threads; the units no
//! saved trace answers are then judged, the one that took longest last time first. What comes out
//! is in exactly the order a serial run gives, whatever the threads did, so the output of a run
//! does not depend on how many it had. With [`THREADS`](crate::reading::THREADS) set to `1` the
//! units are also judged in that order, one after another.

mod gate;
mod units;
mod values;

pub(crate) use gate::{enabled, restamp};

use crate::context::{CrateInput, RepoInput, WorkspaceInput};
use crate::reading::Reading;
use crate::verdict::VerdictCache;
use crate::{LintConfig, LintError, LintPack};

/// What one run lints, and how.
pub struct RunInput<'a> {
    /// The run's one reading, rooted at the repository.
    pub reading:   &'a Reading,
    /// Where saved units live, `None` for a run that keeps nothing.
    pub cache:     Option<&'a dyn VerdictCache>,
    /// The project's `[lints]` configuration, `None` where it has none.
    pub overrides: Option<&'a LintConfig>,
    /// Whether only doc templates changed, which skips every source-only lint.
    pub doc_only:  bool,
    /// What every crate's context shares.
    pub workspace: &'a WorkspaceInput,
    /// Every crate the workspace lints read, in the order findings come out in.
    pub crates:    &'a [CrateInput],
    /// The crates the crate lints run on, by name; `None` for all of [`Self::crates`].
    pub scope:     Option<&'a [String]>,
    /// What the repo lints are handed; `None` runs none.
    pub repo:      Option<RepoInput<'a>>,
    /// Only the lint of this name, of whatever kind; `None` for every lint.
    pub only:      Option<&'a str>,
}

/// Every lint enabled for `input`, built-in and from `pack`, over the units they make, with the
/// configured severities applied: crate lints crate by crate, then workspace lints, then repo
/// lints, each kind built-in first.
///
/// Where a timing report is on ([`crate::timing`]), each lint's judging time is summed under
/// `lint:<name>`, and `units:replayed` and `units:run` count the units a saved trace answered and the
/// units that were judged.
#[must_use]
pub fn run(input: &RunInput<'_>, pack: Option<&LintPack>) -> Vec<LintError> {
    let values = values::RunValues {
        ws:     input.workspace,
        crates: input.crates,
        repo:   input.repo,
    };
    let store = input
        .cache
        .map(|cache| crate::reading::Store::new(input.reading, cache, &values));
    let found = units::Runner::new(input, pack).run(store.as_ref());
    if let Some(s) = &store {
        s.finish();
    }
    found
}
