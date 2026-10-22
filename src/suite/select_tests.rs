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
        fp.shared.insert(name.clone(), own(name));
        let mut full = own(name);
        for d in g.test_closure(name) {
            full.push('+');
            full.push_str(&own(&d));
        }
        fp.base.insert(name.clone(), full.clone());
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

fn live(entries: &[(&str, &[&str])]) -> crate::suite::live::Live {
    entries
        .iter()
        .map(|(b, ts)| (b.to_string(), ts.iter().map(|t| t.to_string()).collect()))
        .collect()
}

#[test]
fn a_heavy_test_whose_binary_is_gone_is_neither_planned_nor_skipped() {
    let fp = fps(&[]);
    let mut h = green_at(&fp);
    let gone = h.flavour_mut("").tests["render"]["drawn"].clone();
    h.flavour_mut("")
        .tests
        .entry("render::example/figure_preview".to_string())
        .or_default()
        .insert("draws".to_string(), gone);
    // The cache is on and both passed at the current fingerprint, so each is a
    // cached skip until the one whose binary is gone is dropped.
    let mut p = plan_for(Mode::Full, true, &fp, &h);
    assert_eq!(p.heavy.len(), 2, "{:?}", p.heavy);
    assert_eq!(p.skip.len(), 2, "{:?}", p.skip);
    p.retain_live(&live(&[("render", &["drawn", "quick"])]));
    assert_eq!(p.heavy, vec![("render".to_string(), "drawn".to_string())]);
    assert_eq!(p.skip.len(), 1, "{:?}", p.skip);
    assert_eq!(
        (p.skip[0].binary.as_str(), p.skip[0].name.as_str()),
        ("render", "drawn")
    );
    let cfg = crate::suite::nextest::tool_config(&Default::default(), &p.heavy, std::path::Path::new("junit.xml"));
    assert!(!cfg.contains("figure_preview"), "{cfg}");
    let filter = crate::suite::nextest::filterset(None, &p.skip, &[]).unwrap();
    assert!(!filter.contains("figure_preview"), "{filter}");
}

#[test]
fn a_heavy_test_that_still_exists_stays_planned() {
    let fp = fps(&[]);
    let h = green_at(&fp);
    let mut p = plan_for(Mode::Full, false, &fp, &h);
    let before = p.clone();
    p.retain_live(&live(&[("render", &["drawn", "quick"])]));
    assert_eq!(p, before);
}

// ---- test binaries ----

/// Fingerprints where `render` has three binaries: its library `render`, and the
/// integration tests `render::a` and `render::b`, each at `@<version>`.
fn with_bins(mut fp: Fingerprints, versions: &[(&str, u32)]) -> Fingerprints {
    let v: BTreeMap<&str, u32> = versions.iter().copied().collect();
    for b in ["render::a", "render::b"] {
        fp.bins.insert(
            b.to_string(),
            format!("{b}@{}", v.get(b).copied().unwrap_or(0)),
        );
    }
    fp
}

/// A history where `render` and `web` are green at `fp` and the three of
/// `render`'s binaries passed there.
fn passed_at(fp: &Fingerprints) -> History {
    let mut h = green_at(fp);
    let f = h.flavour_mut("");
    for b in ["render", "render::a", "render::b"] {
        f.bins
            .insert(b.to_string(), fp.of_binary(b).unwrap().clone());
    }
    h
}

#[test]
fn a_binary_whose_inputs_did_not_move_is_left_out_of_a_member_that_runs() {
    let before = with_bins(fps(&[]), &[]);
    let h = passed_at(&before);
    // `render::b` changed, so the member does, and `a` and the library did not.
    let after = with_bins(fps(&[("render", 1)]), &[("render::b", 1)]);
    let mut after = after;
    after
        .base
        .insert("render".to_string(), before.base["render"].clone());
    let p = plan_for(Mode::Changed, true, &after, &h);
    assert_eq!(p.reuse, vec!["render", "render::a"]);
    assert_eq!(names(&p), vec!["render", "web"]);
}

/// The rule over the whole set of shapes it is stated for: a binary is left out
/// of a run exactly when the cache is on, the run is not a full one, its member
/// runs, and the record is of the fingerprint it has now.
#[test]
fn a_binary_is_reused_exactly_when_every_condition_holds() {
    let before = with_bins(fps(&[]), &[]);
    for mode in [Mode::Changed, Mode::Full, Mode::Cheap] {
        for cache in [true, false] {
            for record in [None, Some("same"), Some("other")] {
                for member_runs in [true, false] {
                    let mut h = green_at(&before);
                    if let Some(r) = record {
                        let at = if r == "same" {
                            before.bins["render::a"].clone()
                        } else {
                            "stale".to_string()
                        };
                        h.flavour_mut("").bins.insert("render::a".to_string(), at);
                    }
                    let fp = if member_runs {
                        with_bins(fps(&[("render", 1)]), &[("render::b", 1)])
                    } else {
                        before.clone()
                    };
                    let p = plan_for(mode, cache, &fp, &h);
                    let want = cache && mode != Mode::Full && record == Some("same") && member_runs;
                    assert_eq!(
                        p.reuse.contains(&"render::a".to_string()),
                        want,
                        "mode {mode:?}, cache {cache}, record {record:?}, member runs {member_runs}"
                    );
                }
            }
        }
    }
}

/// A binary of a member that does not run is the member's business: it is
/// settled whole, and nothing of it is listed as left out.
#[test]
fn only_the_binaries_of_members_that_run_are_listed() {
    let before = with_bins(fps(&[]), &[]);
    let h = passed_at(&before);
    let p = plan_for(Mode::Changed, true, &before, &h);
    assert!(p.run.is_empty());
    assert!(p.reuse.is_empty());
}

/// The heavy tests of a binary that is left out are not planned, and the heavy
/// tests of one that runs are.
#[test]
fn heavy_tests_follow_their_binary() {
    let before = with_bins(fps(&[]), &[]);
    let mut h = passed_at(&before);
    let slow = |at: &str| {
        TestRecord {
            secs:      40.0,
            passed:    true,
            passed_at: Some(at.to_string()),
        }
    };
    let f = h.flavour_mut("");
    f.tests
        .entry("render::a".to_string())
        .or_default()
        .insert("drawn_a".to_string(), slow(&before.bins["render::a"]));
    f.tests
        .entry("render::b".to_string())
        .or_default()
        .insert("drawn_b".to_string(), slow(&before.bins["render::b"]));
    let mut after = with_bins(fps(&[("render", 1)]), &[("render::b", 1)]);
    after
        .base
        .insert("render".to_string(), before.base["render"].clone());
    let p = plan_for(Mode::Changed, true, &after, &h);
    let heavy: Vec<_> = p.heavy.iter().map(|(b, n)| format!("{b} {n}")).collect();
    assert!(
        heavy.contains(&"render::b drawn_b".to_string()),
        "{heavy:?}"
    );
    assert!(!heavy.iter().any(|h| h.contains("drawn_a")), "{heavy:?}");
    assert!(
        p.skip.iter().all(|s| s.binary != "render::a"),
        "{:?}",
        p.skip
    );
}

/// A heavy test's pass stands as long as its own binary's inputs, which can be
/// longer than its member's.
#[test]
fn a_heavy_tests_pass_stands_at_its_binarys_fingerprint_not_its_members() {
    let before = with_bins(fps(&[]), &[]);
    let mut h = green_at(&before);
    h.flavour_mut("")
        .tests
        .entry("render::a".to_string())
        .or_default()
        .insert("drawn_a".to_string(), TestRecord {
            secs:      40.0,
            passed:    true,
            passed_at: Some(before.bins["render::a"].clone()),
        });
    // The member moved (`b` changed) and `a` did not; a full run still asks for
    // `a`, and its heavy test's pass stands.
    let after = with_bins(fps(&[("render", 1)]), &[("render::b", 1)]);
    assert_ne!(after.full["render"], before.full["render"]);
    let p = plan_for(Mode::Full, true, &after, &h);
    assert_eq!(p.skip, vec![Skip {
        binary: "render::a".to_string(),
        name:   "drawn_a".to_string(),
        why:    SkipWhy::Cached,
    }]);
    // And once `a` itself moves it does not.
    let moved_a = with_bins(fps(&[("render", 1)]), &[("render::a", 1)]);
    assert!(plan_for(Mode::Full, true, &moved_a, &h).skip.is_empty());
}

fn pass(bin: &str, outcome: Outcome) -> Case {
    Case {
        binary: bin.to_string(),
        name: "t".to_string(),
        secs: 0.1,
        outcome,
    }
}

#[test]
fn a_run_leaves_each_binary_that_passed_at_its_fingerprint_and_names_each_that_failed() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let cases = vec![
        pass("render", Outcome::Pass),
        pass("render::a", Outcome::Pass),
        pass("render::a", Outcome::Skipped),
        pass("render::b", Outcome::Pass),
        pass("render::b", Outcome::Fail),
        pass("unknown::x", Outcome::Pass),
    ];
    let o = binary_outcomes(&p, &cases, &fp, true);
    assert_eq!(o.passed, vec![
        ("render".to_string(), fp.base["render"].clone()),
        ("render::a".to_string(), fp.bins["render::a"].clone()),
    ]);
    assert_eq!(o.failed, vec!["render::b"]);
    // A binary nothing is known of cannot be recorded, passing or not.
}

#[test]
fn an_incomplete_run_settles_no_binary() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let o = binary_outcomes(
        &p,
        &[pass("render::a", Outcome::Pass), pass("render::b", Outcome::Fail)],
        &fp,
        false,
    );
    assert_eq!(o, BinaryOutcomes::default());
}

/// A binary with a heavy test the run deferred has not run whole.
#[test]
fn a_binary_with_a_deferred_heavy_test_does_not_pass() {
    let fp = with_bins(fps(&[]), &[]);
    let mut p = plan_for(Mode::Full, false, &fp, &History::default());
    p.skip.push(Skip {
        binary: "render::a".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Deferred,
    });
    // A cached one has a standing pass and does not stop it.
    p.skip.push(Skip {
        binary: "render::b".to_string(),
        name:   "drawn".to_string(),
        why:    SkipWhy::Cached,
    });
    let o = binary_outcomes(
        &p,
        &[pass("render::a", Outcome::Pass), pass("render::b", Outcome::Pass)],
        &fp,
        true,
    );
    assert_eq!(
        o.passed.iter().map(|(b, _)| b.as_str()).collect::<Vec<_>>(),
        vec!["render::b"]
    );
}

/// A binary left out of the run is still there after it: a run's record keeps
/// what it did not touch.
#[test]
fn a_gone_binary_is_not_reused() {
    let before = with_bins(fps(&[]), &[]);
    let h = passed_at(&before);
    let mut after = with_bins(fps(&[("render", 1)]), &[("render::b", 1)]);
    after
        .base
        .insert("render".to_string(), before.base["render"].clone());
    let mut p = plan_for(Mode::Changed, true, &after, &h);
    p.retain_live(&live(&[("render", &["q"]), ("render::b", &["q"])]));
    assert_eq!(
        p.reuse,
        vec!["render"],
        "`render::a` is gone, and nextest refuses a filter naming it"
    );
}

/// A failure takes the pass away even at the fingerprint it passed at, which is
/// what a test depending on something no fingerprint holds does; and a pass
/// brings it back. The plan after each is what decides.
#[test]
fn a_failure_at_the_same_inputs_takes_the_pass_away() {
    let fp = with_bins(fps(&[]), &[]);
    let mut h = passed_at(&fp);
    let moved_b = with_bins(fps(&[("render", 1)]), &[("render::b", 1)]);
    let reused = |h: &History| plan_for(Mode::Changed, true, &moved_b, h).reuse;
    assert!(reused(&h).contains(&"render::a".to_string()));

    let failed = BinaryOutcomes {
        passed: vec![],
        failed: vec!["render::a".to_string()],
    };
    failed.apply(h.flavour_mut(""));
    assert!(
        !reused(&h).contains(&"render::a".to_string()),
        "a binary that last failed is rerun"
    );

    let passed = BinaryOutcomes {
        passed: vec![("render::a".to_string(), fp.bins["render::a"].clone())],
        failed: vec![],
    };
    passed.apply(h.flavour_mut(""));
    assert!(reused(&h).contains(&"render::a".to_string()));
}

/// A binary with no test in it never appears in a report, and is settled by
/// holding none rather than by passing one.
#[test]
fn a_binary_holding_no_test_is_settled_with_the_members_that_ran() {
    let fp = with_bins(fps(&[]), &[]);
    let p = plan_for(Mode::Full, false, &fp, &History::default());
    let l = live(&[
        ("render", &[]),
        ("render::a", &["t"]),
        ("render::b", &[]),
        // A member that did not run in this plan is not this plan's to settle.
        ("web", &[]),
    ]);
    let mut p_without_web = p.clone();
    p_without_web.run.retain(|(n, _)| n != "web");
    let idle = idle_binaries(&p_without_web, &l, &fp, true);
    assert_eq!(idle, vec![
        ("render".to_string(), fp.base["render"].clone()),
        ("render::b".to_string(), fp.bins["render::b"].clone()),
    ]);
    assert!(idle_binaries(&p_without_web, &l, &fp, false).is_empty());
}
