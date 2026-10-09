//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What earlier runs established, which is everything the selection and the
//! cache are inferred from.
//!
//! **Tracked in the repository**, under `<mock>/test-history/`, and committed
//! with the work that produced it. A record kept beside the build was lost with
//! every fresh clone and every new container, so each started with a full run
//! and the selection saved nothing where the time was actually going. The
//! fingerprints it keys on are digests of file contents and repository-relative
//! paths, so a pass recorded on one machine stands on another.
//!
//! **One file per member per flavour**, `<flavour>/<member>.json`, so two
//! branches conflict only where both moved the same member, and then either
//! side is as good as the other: the merged tree has a fingerprint neither
//! recorded, and the next run settles it.
//!
//! **Kept small and still.** Only tests at or over half the heavy threshold are
//! recorded, since nothing is decided about the others, and a timing is
//! rewritten only when it crosses the threshold or moves by more than half, so
//! an unchanged suite rerun does not rewrite the files.
//!
//! A flavour is the cargo arguments a run was made with, less the ones choosing
//! packages. A run with `--features editor` compiles other code than one
//! without, so each keeps its own record. Timings are read across flavours when
//! a flavour has none of its own, since a test's weight does not depend much on
//! a feature.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::junit::{Case, Outcome, package_of};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq)]
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
    /// How long it took, to a tenth of a second, as last rewritten.
    pub secs:      f64,
    pub passed:    bool,
    /// Its member's fingerprint when it last passed. A heavy test whose member
    /// still has this fingerprint has nothing new to say.
    pub passed_at: Option<String>,
}

/// One member's record in one flavour, as a file holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct MemberFile {
    version: u32,
    flavour: String,
    member:  String,
    green:   Option<Green>,
    tests:   BTreeMap<String, BTreeMap<String, TestRecord>>,
}

impl History {
    pub fn dir(mock_dir: &Path) -> PathBuf {
        mock_dir.join("test-history")
    }

    /// The record, from every file under the directory. A file that does not
    /// parse, or is of another version, is passed over: a lost record costs one
    /// run of that member, and refusing to test over it would cost more.
    pub fn load(mock_dir: &Path) -> History {
        let mut h = History {
            version: VERSION,
            ..History::default()
        };
        let Ok(flavours) = std::fs::read_dir(Self::dir(mock_dir)) else {
            return h;
        };
        for fdir in flavours.flatten() {
            let Ok(files) = std::fs::read_dir(fdir.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Some(m) = std::fs::read_to_string(file.path())
                    .ok()
                    .and_then(|t| serde_json::from_str::<MemberFile>(&t).ok())
                    .filter(|m| m.version == VERSION)
                else {
                    continue;
                };
                let f = h.flavour_mut(&m.flavour);
                if let Some(g) = m.green {
                    f.green.insert(m.member.clone(), g);
                }
                for (bin, tests) in m.tests {
                    f.tests.entry(bin).or_default().extend(tests);
                }
            }
        }
        h
    }

    /// Writes each member's file where its content changed, and only there, so
    /// a run that settled nothing leaves the tree as it was.
    pub fn save(&self, mock_dir: &Path) -> Result<(), String> {
        for (flavour, f) in &self.flavours {
            let mut members: BTreeMap<String, MemberFile> = BTreeMap::new();
            let blank = |member: &str| {
                MemberFile {
                    version: VERSION,
                    flavour: flavour.clone(),
                    member: member.to_string(),
                    ..MemberFile::default()
                }
            };
            for (member, g) in &f.green {
                members
                    .entry(member.clone())
                    .or_insert_with(|| blank(member))
                    .green = Some(g.clone());
            }
            for (bin, tests) in &f.tests {
                let member = package_of(bin);
                members
                    .entry(member.to_string())
                    .or_insert_with(|| blank(member))
                    .tests
                    .insert(bin.clone(), tests.clone());
            }
            let dir = Self::dir(mock_dir).join(flavour_dir(flavour));
            // A member with nothing left to record loses its file, or a green
            // record a failure took away would load back from it.
            if let Ok(existing) = std::fs::read_dir(&dir) {
                for e in existing.flatten() {
                    let p = e.path();
                    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned());
                    if p.extension().is_some_and(|x| x == "json")
                        && stem.is_some_and(|s| !members.contains_key(&s))
                    {
                        std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    }
                }
            }
            for (member, m) in members {
                let path = dir.join(format!("{member}.json"));
                let mut text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
                text.push('\n');
                if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
                    continue;
                }
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                // Beside itself and renamed over, so an interrupted write
                // leaves the previous record rather than half of one.
                let tmp = dir.join(format!(".{member}.json.tmp"));
                std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
                std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        Ok(())
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
    /// for this run; `threshold` is what counts as heavy.
    pub fn record_cases(
        &mut self,
        flavour: &str,
        cases: &[Case],
        fingerprint: &BTreeMap<String, String>,
        threshold: f64,
    ) {
        let f = self.flavour_mut(flavour);
        for c in cases {
            if c.outcome == Outcome::Skipped {
                continue;
            }
            let tests = f.tests.entry(c.binary.clone()).or_default();
            // Nothing is decided about a quick test, so it is not kept.
            if c.secs < threshold / 2.0 {
                tests.remove(&c.name);
                continue;
            }
            let passed = c.outcome == Outcome::Pass;
            let secs = match tests.get(&c.name) {
                Some(old) if steady(old.secs, c.secs, threshold) => old.secs,
                _ => tenths(c.secs),
            };
            tests.insert(c.name.clone(), TestRecord {
                secs,
                passed,
                passed_at: if passed { fingerprint.get(c.package()).cloned() } else { None },
            });
        }
        f.tests.retain(|_, t| !t.is_empty());
    }
}

/// Whether a new timing says nothing the old one did not: on the same side of
/// the threshold and within half again either way.
fn steady(old: f64, new: f64, threshold: f64) -> bool {
    (old >= threshold) == (new >= threshold) && new <= old * 1.5 && new >= old / 1.5
}

fn tenths(secs: f64) -> f64 {
    (secs * 10.0).round() / 10.0
}

/// The directory a flavour's files sit in: `default` for none, else its
/// arguments with everything but letters, digits and dashes made a dash.
fn flavour_dir(flavour: &str) -> String {
    let mut out = String::new();
    for ch in flavour.trim().chars() {
        let ch = if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' };
        if !(ch == '-' && (out.is_empty() || out.ends_with('-'))) {
            out.push(ch);
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "default".to_string() } else { out }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
