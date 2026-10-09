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
use super::junit::{Case, Outcome};

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
    p.heavy = i
        .history
        .heavy(i.flavour, i.threshold)
        .into_iter()
        .filter(|(bin, _)| running.contains(super::junit::package_of(bin)))
        .collect();

    for (bin, name) in &p.heavy {
        let pkg = super::junit::package_of(bin);
        let record = f.tests.get(bin).and_then(|t| t.get(name));
        let at = i.fp.full.get(pkg);
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

/// Which own inputs moved under a member since its suite last passed.
fn moved(name: &str, g: &Green, fp: &Fingerprints, graph: &Graph) -> Why {
    let mut who: Vec<String> = std::iter::once(name.to_string())
        .chain(graph.test_closure(name))
        .filter(|m| fp.own.get(m) != g.own.get(m))
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
        .map(|s| super::junit::package_of(&s.binary))
        .collect();
    plan.run
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| !failed.contains(n) && !deferred.contains(n))
        .map(str::to_string)
        .collect()
}

/// The record a green member keeps: its fingerprint, and the own inputs of it
/// and its closure.
pub fn green_record(name: &str, fp: &Fingerprints, graph: &Graph) -> Green {
    let own: BTreeMap<String, String> = std::iter::once(name.to_string())
        .chain(graph.test_closure(name))
        .filter_map(|m| fp.own.get(&m).map(|o| (m.clone(), o.clone())))
        .collect();
    Green {
        full: fp.full.get(name).cloned().unwrap_or_default(),
        own,
    }
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
