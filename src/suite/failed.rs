//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The members whose tests failed the last time they ran, which is what the
//! next run builds with symbols ([`super::debug`]).
//!
//! **Kept beside the build, not in the tracked history.** The history says what
//! passed at which fingerprint, and a fresh clone is the better for it. A
//! failure is the opposite: it belongs to one working copy at one moment, and
//! committed it would hand every other clone symbols for a crate that is red
//! only on one machine, and put a record the merge driver has to settle into
//! every branch that happened to go red. A target directory that is gone
//! costs one run without symbols for a crate that fails again.
//!
//! Per flavour, since a `--features` run compiles other code and fails in
//! other places.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::junit::{Case, Outcome};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Failed {
    #[serde(default)]
    flavours: BTreeMap<String, BTreeSet<String>>,
}

impl Failed {
    pub fn path(state: &Path) -> PathBuf {
        state.join("failed.json")
    }

    /// The record, or nothing known where there is none or it does not read:
    /// a lost record costs one run without symbols.
    pub fn load(state: &Path) -> Failed {
        std::fs::read_to_string(Self::path(state))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, state: &Path) -> Result<(), String> {
        let path = Self::path(state);
        let mut text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        text.push('\n');
        // Beside itself and renamed over, so a run cut short leaves the earlier
        // record and not half of one.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The members that failed last time in this flavour.
    pub fn of(&self, flavour: &str) -> BTreeSet<String> {
        self.flavours.get(flavour).cloned().unwrap_or_default()
    }

    /// Takes `members` off the record for this flavour: a plain run that passed
    /// them, which reads no report and so cannot say which tests it ran, passed
    /// the whole tree. Whether anything changed.
    pub fn clear(&mut self, flavour: &str, members: &[String]) -> bool {
        let Some(set) = self.flavours.get_mut(flavour) else {
            return false;
        };
        let before = set.len();
        set.retain(|m| !members.contains(m));
        let changed = set.len() != before;
        if set.is_empty() {
            self.flavours.remove(flavour);
        }
        changed
    }

    /// What a run settled. A member that failed a test is recorded. A member
    /// that ran and had no failure is cleared when `whole` says the run was not
    /// cut short or narrowed, since a pass of some of its tests says nothing
    /// about the one that failed. A member that ran no test, cached or left
    /// out, keeps what it had.
    pub fn settle(&mut self, flavour: &str, ran: &[String], cases: &[Case], whole: bool) {
        let failing: BTreeSet<&str> = cases
            .iter()
            .filter(|c| c.outcome == Outcome::Fail)
            .map(Case::package)
            .collect();
        let tested: BTreeSet<&str> = cases.iter().map(Case::package).collect();
        let set = self.flavours.entry(flavour.to_string()).or_default();
        if whole {
            set.retain(|m| !(ran.contains(m) && tested.contains(m.as_str())));
        }
        set.extend(failing.iter().map(|m| m.to_string()));
        if set.is_empty() {
            self.flavours.remove(flavour);
        }
    }
}

#[cfg(test)]
#[path = "failed_tests.rs"]
mod tests;
