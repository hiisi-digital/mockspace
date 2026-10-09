//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What earlier runs established, which is everything the selection and the
//! cache are inferred from.
//!
//! Kept per flavour: the cargo arguments a run was made with, less the ones
//! choosing packages. A run with `--features editor` compiles other code than
//! one without, so a member green without the feature says nothing about it
//! with, and each keeps its own record. Timings are read across flavours when a
//! flavour has none of its own, since a test's weight does not depend much on a
//! feature.
//!
//! Lives under the mock directory's `target/`, beside the build it describes,
//! and is lost with it: a `cargo clean` or a fresh clone runs everything once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::junit::{Case, Outcome};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub version:  u32,
    pub flavours: BTreeMap<String, Flavour>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Flavour {
    /// Each member at the fingerprint its whole suite last passed at.
    pub green: BTreeMap<String, Green>,
    /// Each test by binary, then by name.
    pub tests: BTreeMap<String, BTreeMap<String, TestRecord>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Green {
    /// The member's fingerprint when its suite last passed whole.
    pub full: String,
    /// The own inputs of it and of everything in its closure at that moment,
    /// so a later run can say which of them moved rather than only that one did.
    pub own:  BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestRecord {
    /// How long it took the last time it ran.
    pub secs:      f64,
    pub passed:    bool,
    /// Its member's fingerprint when it last passed. A heavy test whose member
    /// still has this fingerprint has nothing new to say.
    pub passed_at: Option<String>,
}

impl History {
    pub fn path(mock_dir: &Path) -> PathBuf {
        mock_dir
            .join("target")
            .join("mockspace-test-state")
            .join("history.json")
    }

    /// The record, or an empty one when there is none or it is of another
    /// version. Never an error: a lost history costs one full run and nothing
    /// else, and refusing to test over it would cost more.
    pub fn load(mock_dir: &Path) -> History {
        std::fs::read_to_string(Self::path(mock_dir))
            .ok()
            .and_then(|t| serde_json::from_str::<History>(&t).ok())
            .filter(|h| h.version == VERSION)
            .unwrap_or_else(|| {
                History {
                    version: VERSION,
                    ..History::default()
                }
            })
    }

    /// Written whole beside itself and renamed over, so an interrupted write
    /// leaves the previous record rather than half of one.
    pub fn save(&self, mock_dir: &Path) -> Result<(), String> {
        let path = Self::path(mock_dir);
        let dir = path.parent().expect("history has a parent");
        crate::build_dir::ensure(dir.to_path_buf());
        let tmp = dir.join("history.json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn flavour(&self, key: &str) -> Option<&Flavour> {
        self.flavours.get(key)
    }

    pub fn flavour_mut(&mut self, key: &str) -> &mut Flavour {
        self.flavours.entry(key.to_string()).or_default()
    }

    /// How long a test last took: in this flavour, else the longest any other
    /// flavour saw, else nothing known.
    pub fn secs(&self, flavour: &str, binary: &str, name: &str) -> Option<f64> {
        let own = self
            .flavour(flavour)
            .and_then(|f| f.tests.get(binary))
            .and_then(|t| t.get(name))
            .map(|r| r.secs);
        own.or_else(|| {
            self.flavours
                .values()
                .filter_map(|f| f.tests.get(binary).and_then(|t| t.get(name)))
                .map(|r| r.secs)
                .reduce(f64::max)
        })
    }

    /// Every test known heavy for this flavour, as (binary, name).
    pub fn heavy(&self, flavour: &str, threshold: f64) -> Vec<(String, String)> {
        let mut seen: BTreeMap<(String, String), f64> = BTreeMap::new();
        for f in self.flavours.values() {
            for (bin, tests) in &f.tests {
                for name in tests.keys() {
                    if let Some(s) = self.secs(flavour, bin, name) {
                        seen.insert((bin.clone(), name.clone()), s);
                    }
                }
            }
        }
        seen.into_iter()
            .filter(|(_, s)| *s >= threshold)
            .map(|(k, _)| k)
            .collect()
    }

    /// Folds one run's results in. `fingerprint` is each member's fingerprint
    /// for this run.
    pub fn record_cases(
        &mut self,
        flavour: &str,
        cases: &[Case],
        fingerprint: &BTreeMap<String, String>,
    ) {
        let f = self.flavour_mut(flavour);
        for c in cases {
            if c.outcome == Outcome::Skipped {
                continue;
            }
            let passed = c.outcome == Outcome::Pass;
            let at = fingerprint.get(c.package()).cloned();
            let entry = f
                .tests
                .entry(c.binary.clone())
                .or_default()
                .entry(c.name.clone())
                .or_insert(TestRecord {
                    secs: c.secs,
                    passed,
                    passed_at: None,
                });
            entry.secs = c.secs;
            entry.passed = passed;
            entry.passed_at = if passed { at } else { None };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(bin: &str, name: &str, secs: f64, outcome: Outcome) -> Case {
        Case {
            binary: bin.to_string(),
            name: name.to_string(),
            secs,
            outcome,
        }
    }

    fn fps() -> BTreeMap<String, String> {
        BTreeMap::from([("render".to_string(), "fp1".to_string())])
    }

    #[test]
    fn a_pass_records_its_fingerprint_and_a_failure_clears_it() {
        let mut h = History::default();
        h.record_cases("", &[case("render", "drawn", 40.0, Outcome::Pass)], &fps());
        let r = &h.flavours[""].tests["render"]["drawn"];
        assert_eq!(
            (r.secs, r.passed, r.passed_at.as_deref()),
            (40.0, true, Some("fp1"))
        );

        h.record_cases("", &[case("render", "drawn", 41.0, Outcome::Fail)], &fps());
        let r = &h.flavours[""].tests["render"]["drawn"];
        assert_eq!(
            (r.secs, r.passed, r.passed_at.as_deref()),
            (41.0, false, None)
        );
    }

    /// An ignored test did not run, and what it took to not run is no timing.
    #[test]
    fn a_skipped_case_leaves_the_record_alone() {
        let mut h = History::default();
        h.record_cases("", &[case("render", "drawn", 40.0, Outcome::Pass)], &fps());
        h.record_cases(
            "",
            &[case("render", "drawn", 0.0, Outcome::Skipped)],
            &fps(),
        );
        assert_eq!(h.flavours[""].tests["render"]["drawn"].secs, 40.0);
    }

    #[test]
    fn heavy_is_at_or_over_the_threshold_and_borrows_across_flavours() {
        let mut h = History::default();
        h.record_cases(
            "",
            &[
                case("render", "drawn", 40.0, Outcome::Pass),
                case("render", "edge", 10.0, Outcome::Pass),
                case("render", "quick", 0.01, Outcome::Pass),
            ],
            &fps(),
        );
        let names = |v: Vec<(String, String)>| v.into_iter().map(|(_, n)| n).collect::<Vec<_>>();
        assert_eq!(names(h.heavy("", 10.0)), vec!["drawn", "edge"]);
        // The editor flavour has never run; it borrows the default's timings.
        assert_eq!(names(h.heavy("--features editor", 10.0)), vec![
            "drawn", "edge"
        ]);
        // Once it has run, its own timing decides, here that `drawn` got quick.
        h.record_cases(
            "--features editor",
            &[case("render", "drawn", 1.0, Outcome::Pass)],
            &fps(),
        );
        assert_eq!(names(h.heavy("--features editor", 10.0)), vec!["edge"]);
    }

    #[test]
    fn a_saved_history_loads_back_and_a_foreign_one_loads_empty() {
        let dir = std::env::temp_dir().join(format!("mockspace-suite-hist-{}", std::process::id()));
        let mut h = History::load(&dir);
        assert_eq!(h.version, VERSION);
        h.record_cases("", &[case("render", "drawn", 40.0, Outcome::Pass)], &fps());
        h.save(&dir).unwrap();
        assert_eq!(History::load(&dir), h);

        std::fs::write(History::path(&dir), r#"{"version": 999, "flavours": {}}"#).unwrap();
        assert!(History::load(&dir).flavours.is_empty());
        std::fs::write(History::path(&dir), "not json").unwrap();
        assert!(History::load(&dir).flavours.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
