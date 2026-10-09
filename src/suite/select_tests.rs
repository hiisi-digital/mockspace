//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::*;
use crate::suite::graph::{DepKind, Package};
use crate::suite::history::TestRecord;

/// `mesh` <- `render` <- `web`, and `tool` alone.
fn graph() -> Graph {
    let pkg = |name: &str, deps: &[&str]| {
        Package {
            name: name.to_string(),
            dir:  PathBuf::from("/w").join(name),
            deps: deps
                .iter()
                .map(|d| (d.to_string(), DepKind::Normal))
                .collect(),
        }
    };
    let mut g = Graph::default();
    for p in [
        pkg("mesh", &[]),
        pkg("render", &["mesh"]),
        pkg("web", &["render"]),
        pkg("tool", &[]),
    ] {
        g.packages.insert(p.name.clone(), p);
    }
    g
}

/// Fingerprints where each member's own input is `<name>@<version>` and its
/// full one folds in its closure's, the way `compute` does.
fn fps(versions: &[(&str, u32)]) -> Fingerprints {
    let g = graph();
    let v: BTreeMap<&str, u32> = versions.iter().copied().collect();
    let own = |n: &str| format!("{n}@{}", v.get(n).copied().unwrap_or(0));
    let mut fp = Fingerprints::default();
    for name in g.packages.keys() {
        fp.own.insert(name.clone(), own(name));
        let mut full = own(name);
        for d in g.test_closure(name) {
            full.push('+');
            full.push_str(&own(&d));
        }
        fp.full.insert(name.clone(), full);
    }
    fp
}

fn all() -> Vec<String> {
    ["mesh", "render", "tool", "web"].map(String::from).to_vec()
}

/// A history where every member went green at `fp`, and `render` has one
/// heavy test that passed there and one quick one.
fn green_at(fp: &Fingerprints) -> History {
    let g = graph();
    let mut h = History::default();
    let f = h.flavour_mut("");
    for n in all() {
        f.green.insert(n.clone(), green_record(&n, fp, &g));
    }
    let tests = f.tests.entry("render".to_string()).or_default();
    tests.insert("drawn".to_string(), TestRecord {
        secs:      40.0,
        passed:    true,
        passed_at: Some(fp.full["render"].clone()),
    });
    tests.insert("quick".to_string(), TestRecord {
        secs:      0.01,
        passed:    true,
        passed_at: Some(fp.full["render"].clone()),
    });
    h
}

fn plan_for(mode: Mode, cache: bool, fp: &Fingerprints, h: &History) -> Plan {
    let candidates = all();
    let g = graph();
    plan(&Inputs {
        mode,
        cache,
        candidates: &candidates,
        fp,
        graph: &g,
        history: h,
        flavour: "",
        threshold: 10.0,
    })
}

fn names(p: &Plan) -> Vec<&str> {
    p.run.iter().map(|(n, _)| n.as_str()).collect()
}

#[test]
fn with_no_history_everything_runs() {
    let fp = fps(&[]);
    let p = plan_for(Mode::Changed, true, &fp, &History::default());
    assert_eq!(names(&p), vec!["mesh", "render", "tool", "web"]);
    assert!(p.run.iter().all(|(_, w)| *w == Why::NeverGreen));
    assert!(
        p.heavy.is_empty(),
        "nothing is known heavy before a run has timed it"
    );
}

#[test]
fn after_none_nothing_runs() {
    let fp = fps(&[]);
    let p = plan_for(Mode::Changed, true, &fp, &green_at(&fp));
    assert!(p.run.is_empty());
    assert_eq!(p.settled, all());
}

/// The one-line change in a crate the suites depend on.
#[test]
fn a_dependency_moving_selects_it_and_its_dependents_saying_why() {
    let before = fps(&[]);
    let h = green_at(&before);
    let after = fps(&[("mesh", 1)]);
    let p = plan_for(Mode::Changed, true, &after, &h);
    assert_eq!(p.run, vec![
        ("mesh".to_string(), Why::Moved(vec!["mesh".to_string()])),
        ("render".to_string(), Why::Moved(vec!["mesh".to_string()])),
        ("web".to_string(), Why::Moved(vec!["mesh".to_string()])),
    ]);
    assert_eq!(p.settled, vec!["tool"]);
    // `drawn` passed at the old fingerprint, which no longer stands.
    assert_eq!(p.heavy, vec![("render".to_string(), "drawn".to_string())]);
    assert!(p.skip.is_empty());
}

#[test]
fn a_workspace_input_moving_is_named_as_that() {
    let fp = fps(&[]);
    let h = green_at(&fp);
    let mut after = fp.clone();
    for v in after.full.values_mut() {
        v.push_str("+lock2");
    }
    let p = plan_for(Mode::Changed, true, &after, &h);
    assert!(
        p.run.iter().all(|(_, w)| *w == Why::Workspace),
        "{:?}",
        p.run
    );
}

/// A full run asks for every member and still skips a heavy test whose pass
/// stands; with the cache off it skips nothing.
#[test]
fn a_full_run_runs_everyone_and_the_cache_still_holds() {
    let fp = fps(&[]);
    let h = green_at(&fp);
    let p = plan_for(Mode::Full, true, &fp, &h);
    assert_eq!(names(&p), vec!["mesh", "render", "tool", "web"]);
    assert_eq!(p.skip, vec![Skip {
        binary: "render".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Cached,
    }]);
    assert!(plan_for(Mode::Full, false, &fp, &h).skip.is_empty());
}

/// The cheap pass defers a heavy test that has passed before, even though its
/// member moved; a heavy test that failed last time runs.
#[test]
fn a_cheap_run_defers_known_heavy_tests_and_runs_failing_ones() {
    let before = fps(&[]);
    let mut h = green_at(&before);
    let after = fps(&[("render", 1)]);
    let p = plan_for(Mode::Cheap, true, &after, &h);
    assert_eq!(p.skip, vec![Skip {
        binary: "render".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Deferred,
    }]);

    let r = h
        .flavour_mut("")
        .tests
        .get_mut("render")
        .unwrap()
        .get_mut("drawn")
        .unwrap();
    r.passed = false;
    r.passed_at = None;
    assert!(plan_for(Mode::Cheap, true, &after, &h).skip.is_empty());
}

#[test]
fn a_complete_run_leaves_green_what_passed_and_nothing_deferred() {
    let fp = fps(&[("render", 1)]);
    let mut p = plan_for(Mode::Full, false, &fp, &History::default());
    let case = |bin: &str, outcome| {
        Case {
            binary: bin.to_string(),
            name: "t".to_string(),
            secs: 0.1,
            outcome,
        }
    };
    let cases = vec![
        case("mesh", Outcome::Pass),
        case("render", Outcome::Pass),
        case("web::it", Outcome::Fail),
    ];
    // `tool` has no tests at all, and passes vacuously.
    assert_eq!(greens(&p, &cases, true), vec!["mesh", "render", "tool"]);
    assert!(
        greens(&p, &cases, false).is_empty(),
        "an incomplete run leaves nothing green"
    );

    p.skip.push(Skip {
        binary: "render".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Deferred,
    });
    assert_eq!(greens(&p, &cases, true), vec!["mesh", "tool"]);
}
