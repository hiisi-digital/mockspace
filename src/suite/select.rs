//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which members run, which heavy tests in them do not, and which members a
//! finished run leaves green. Pure: everything it decides from is handed in,
//! so every rule here is a test away.

use std::collections::{BTreeMap, BTreeSet};

use super::fingerprint::Fingerprints;
use super::graph::Graph;
use super::history::{Flavour, Green, History};
use super::junit::{Case, Outcome, package_of};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The members something they depend on has moved under since their suite
    /// last passed, heavy tests included unless their last pass still stands.
    Changed,
    /// Every member asked for, whatever moved. Heavy tests whose last pass
    /// still stands are still skipped, unless the cache is off too.
    Full,
    /// As `Changed`, with every heavy test skipped that has run before in this
    /// flavour and did not fail. New heavy tests, and ones that failed last
    /// time, still run. The members it reaches are not left green, since their
    /// heavy tests are owed.
    Cheap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The run asked for every member.
    Asked,
    /// No record of its suite ever passing in this flavour.
    NeverGreen,
    /// These members' own inputs moved since its suite last passed: itself,
    /// what it depends on, or both.
    Moved(Vec<String>),
    /// Nothing of its own or its closure's moved, so what moved is the
    /// workspace's: the lockfile, the toolchain, cargo's configuration.
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipWhy {
    /// It passed at its member's current fingerprint.
    Cached,
    /// A cheap run leaves it for the next full one.
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub binary: String,
    pub name:   String,
    pub why:    SkipWhy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub run:     Vec<(String, Why)>,
    /// Members asked for and not run, since nothing they depend on moved.
    pub settled: Vec<String>,
    /// Every heavy test known in the members that run, as (binary, name).
    pub heavy:   Vec<(String, String)>,
    pub skip:    Vec<Skip>,
    /// Test binaries of the members that run which are left out, since their
    /// inputs have not moved since they last ran whole and passed.
    pub reuse:   Vec<String>,
}

impl Plan {
    /// Drops the heavy tests that no longer exist, whichever flavour
    /// remembered them, so none reaches the generated config or the skips.
    pub fn retain_live(&mut self, live: &super::live::Live) {
        let exists = |bin: &str, name: &str| live.get(bin).is_some_and(|t| t.contains(name));
        self.heavy.retain(|(b, n)| exists(b, n));
        self.skip.retain(|s| exists(&s.binary, &s.name));
        self.reuse.retain(|b| live.contains_key(b));
    }
}

pub struct Inputs<'a> {
    pub mode:       Mode,
    pub cache:      bool,
    pub candidates: &'a [String],
    pub fp:         &'a Fingerprints,
    pub graph:      &'a Graph,
    pub history:    &'a History,
    pub flavour:    &'a str,
    pub threshold:  f64,
}

pub fn plan(i: &Inputs) -> Plan {
    let empty = Flavour::default();
    let f = i.history.flavour(i.flavour).unwrap_or(&empty);
    let mut p = Plan::default();

    for name in i.candidates {
        let now = i.fp.full.get(name);
        let green = f.green.get(name);
        if i.mode == Mode::Full {
            p.run.push((name.clone(), Why::Asked));
            continue;
        }
        match green {
            Some(g) if Some(&g.full) == now => p.settled.push(name.clone()),
            None => p.run.push((name.clone(), Why::NeverGreen)),
            Some(g) => p.run.push((name.clone(), moved(name, g, i.fp, i.graph))),
        }
    }

    let running: BTreeSet<&str> = p.run.iter().map(|(n, _)| n.as_str()).collect();

    // A binary of a member that runs is left out when it ran whole and passed at
    // the inputs it has now. A full run asks for every binary, and without the
    // cache nothing is taken on an earlier pass.
    if i.cache && i.mode != Mode::Full {
        p.reuse = f
            .bins
            .iter()
            .filter(|(bin, at)| {
                running.contains(package_of(bin)) && i.fp.of_binary(bin) == Some(*at)
            })
            .map(|(bin, _)| bin.clone())
            .collect();
    }
    let reused: BTreeSet<&str> = p.reuse.iter().map(String::as_str).collect();

    p.heavy = i
        .history
        .heavy(i.flavour, i.threshold)
        .into_iter()
        .filter(|(bin, _)| running.contains(package_of(bin)) && !reused.contains(bin.as_str()))
        .collect();

    for (bin, name) in &p.heavy {
        let record = f.tests.get(bin).and_then(|t| t.get(name));
        let at = i.fp.of_binary(bin);
        let why = if i.cache && at.is_some() && record.and_then(|r| r.passed_at.as_ref()) == at {
            Some(SkipWhy::Cached)
        } else if i.mode == Mode::Cheap && record.is_none_or(|r| r.passed) {
            Some(SkipWhy::Deferred)
        } else {
            None
        };
        if let Some(why) = why {
            p.skip.push(Skip {
                binary: bin.clone(),
                name: name.clone(),
                why,
            });
        }
    }
    p
}

/// What a member's fingerprint holds of `m` when `m` is `name` itself (its own
/// inputs) or something its tests are built from (that member's shared inputs).
fn input_of<'a>(fp: &'a Fingerprints, name: &str, m: &str) -> Option<&'a String> {
    if m == name { fp.own.get(m) } else { fp.shared.get(m) }
}

/// Which inputs moved under a member since its suite last passed.
fn moved(name: &str, g: &Green, fp: &Fingerprints, graph: &Graph) -> Why {
    let mut who: Vec<String> = std::iter::once(name.to_string())
        .chain(graph.test_closure(name))
        .filter(|m| input_of(fp, name, m) != g.own.get(m))
        .collect();
    who.sort();
    if who.is_empty() { Why::Workspace } else { Why::Moved(who) }
}

/// The members a finished run leaves green: those it ran whose every case
/// passed, none of whose heavy tests it deferred.
///
/// `complete` says every test the run selected did run: nextest finished with
/// success or with test failures, rather than on a build error, an interrupt
/// or a timeout that stopped the run, and no filter of the caller's narrowed
/// it. An incomplete run leaves nothing green, since a member missing from the
/// report looks exactly like one with nothing to report.
pub fn greens(plan: &Plan, cases: &[Case], complete: bool) -> Vec<String> {
    if !complete {
        return Vec::new();
    }
    let failed: BTreeSet<&str> = cases
        .iter()
        .filter(|c| c.outcome == Outcome::Fail)
        .map(|c| c.package())
        .collect();
    let deferred: BTreeSet<&str> = plan
        .skip
        .iter()
        .filter(|s| s.why == SkipWhy::Deferred)
        .map(|s| package_of(&s.binary))
        .collect();
    plan.run
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| !failed.contains(n) && !deferred.contains(n))
        .map(str::to_string)
        .collect()
}

/// The record a green member keeps: its fingerprint, its own inputs and the
/// shared inputs of its closure.
pub fn green_record(name: &str, fp: &Fingerprints, graph: &Graph) -> Green {
    let own: BTreeMap<String, String> = std::iter::once(name.to_string())
        .chain(graph.test_closure(name))
        .filter_map(|m| input_of(fp, name, &m).map(|o| (m.clone(), o.clone())))
        .collect();
    Green {
        full: fp.full.get(name).cloned().unwrap_or_default(),
        own,
    }
}

/// What a finished run settles about each test binary that ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinaryOutcomes {
    /// Binaries that ran and passed, with the fingerprint they passed at.
    pub passed: Vec<(String, String)>,
    /// Binaries in which a test failed, whose record of a pass is no longer true.
    pub failed: Vec<String>,
}

impl BinaryOutcomes {
    /// Folds the run into the record: a binary that failed loses its pass, even
    /// one made at the fingerprint it failed at (a test that depends on
    /// something no fingerprint holds), and one that passed gains it.
    pub fn apply(self, record: &mut Flavour) {
        for bin in &self.failed {
            record.bins.remove(bin);
        }
        record.bins.extend(self.passed);
    }
}

/// The binaries a run leaves passed or failed, from its report.
///
/// A binary passed when it appears in the report, none of its cases failed and
/// the run deferred none of its heavy tests; the heavy tests a cache skipped
/// passed at the same fingerprint, so they stand. `complete` is as for
/// [`greens`]: an incomplete run settles nothing, since a binary missing from
/// its report looks exactly like one that was never started. A binary that
/// failed loses its record even though it is rebuilt on different inputs than
/// the record named, so a stale pass cannot outlive a failure.
pub fn binary_outcomes(
    plan: &Plan,
    cases: &[Case],
    fp: &Fingerprints,
    complete: bool,
) -> BinaryOutcomes {
    let mut out = BinaryOutcomes::default();
    if !complete {
        return out;
    }
    let deferred: BTreeSet<&str> = plan
        .skip
        .iter()
        .filter(|s| s.why == SkipWhy::Deferred)
        .map(|s| s.binary.as_str())
        .collect();
    let mut verdict: BTreeMap<&str, bool> = BTreeMap::new();
    for c in cases {
        let ok = verdict.entry(c.binary.as_str()).or_insert(true);
        *ok &= c.outcome != Outcome::Fail;
    }
    for (bin, ok) in verdict {
        if !ok {
            out.failed.push(bin.to_string());
        } else if !deferred.contains(bin)
            && let Some(at) = fp.of_binary(bin)
        {
            out.passed.push((bin.to_string(), at.clone()));
        }
    }
    out
}

/// The binaries of the members that ran which hold no test at all, with the
/// fingerprint they stand at. Nothing of theirs appears in a report, so without
/// this they would be started again by every run to find nothing in them.
pub fn idle_binaries(
    plan: &Plan,
    live: &super::live::Live,
    fp: &Fingerprints,
    complete: bool,
) -> Vec<(String, String)> {
    if !complete {
        return Vec::new();
    }
    let running: BTreeSet<&str> = plan.run.iter().map(|(n, _)| n.as_str()).collect();
    live.iter()
        .filter(|(bin, tests)| tests.is_empty() && running.contains(package_of(bin)))
        .filter_map(|(bin, _)| fp.of_binary(bin).map(|at| (bin.clone(), at.clone())))
        .collect()
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
