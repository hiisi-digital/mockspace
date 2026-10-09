//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a run says before and after: which members run and why, which heavy
//! tests it skips and why, and which tests were slow.

use std::fmt::Write as _;

use super::args::Invocation;
use super::fingerprint::Fingerprints;
use super::junit::{Case, Outcome};
use super::select::{Mode, Plan, SkipWhy, Why};
use crate::config::Config;

/// How many of the slowest tests the closing report names.
const SLOWEST: usize = 10;
/// How many skipped tests are named before the rest are counted.
const NAMED_SKIPS: usize = 12;

pub fn plan(p: &Plan, inv: &Invocation, fp: &Fingerprints, cfg: &Config) {
    print!(
        "{}",
        plan_text(
            p,
            inv,
            fp,
            cfg.test.heavy_after_secs,
            cfg.test.heavy_threads
        )
    );
}

pub fn plan_text(
    p: &Plan,
    inv: &Invocation,
    fp: &Fingerprints,
    heavy_after: f64,
    threads: u32,
) -> String {
    let mut t = String::new();
    let mode = match inv.mode {
        Mode::Changed => "what changed",
        Mode::Full => "full",
        Mode::Cheap => "cheap, heavy tests deferred",
    };
    let flavour = if inv.flavour().is_empty() {
        String::new()
    } else {
        format!(", {}", inv.flavour())
    };
    let _ = writeln!(
        t,
        "\n=== members : nextest, {mode}{flavour}{} ===",
        if inv.cache { "" } else { ", cache off" }
    );
    for (name, why) in &p.run {
        let why = match why {
            Why::Asked => "asked for".to_string(),
            Why::NeverGreen => "no earlier pass on record".to_string(),
            Why::Workspace => "the lockfile, toolchain or cargo configuration moved".to_string(),
            Why::Moved(who) if who.len() == 1 && who[0] == *name => {
                "its own inputs moved".to_string()
            },
            Why::Moved(who) => format!("moved: {}", who.join(", ")),
        };
        let reaches = fp.reaches.get(name).map(Vec::len).unwrap_or(0);
        let reaches = if reaches == 0 {
            String::new()
        } else {
            format!(" (reads {reaches} path(s) outside itself)")
        };
        let _ = writeln!(t, "    run      {name}: {why}{reaches}");
    }
    if !p.settled.is_empty() {
        let _ = writeln!(
            t,
            "    settled  {} member(s) passed at what they are now: {}",
            p.settled.len(),
            p.settled.join(", ")
        );
    }
    if !p.heavy.is_empty() {
        let _ = writeln!(
            t,
            "    heavy    {} test(s) took {heavy_after}s or more last time; {threads} at a time",
            p.heavy.len()
        );
    }
    for (why, label) in [(SkipWhy::Cached, "cached"), (SkipWhy::Deferred, "deferred")] {
        let names: Vec<String> = p
            .skip
            .iter()
            .filter(|s| s.why == why)
            .map(|s| format!("{} {}", s.binary, s.name))
            .collect();
        if names.is_empty() {
            continue;
        }
        let note = match why {
            SkipWhy::Cached => "passed at their member's current fingerprint",
            SkipWhy::Deferred => "owed to the next full or changed run",
        };
        let _ = writeln!(
            t,
            "    {label:<8} {} heavy test(s) not run, {note}:",
            names.len()
        );
        for n in names.iter().take(NAMED_SKIPS) {
            let _ = writeln!(t, "               {n}");
        }
        if names.len() > NAMED_SKIPS {
            let _ = writeln!(t, "               and {} more", names.len() - NAMED_SKIPS);
        }
    }
    t
}

pub fn outcome(p: &Plan, cases: &[Case], heavy_after: f64) {
    print!("{}", outcome_text(p, cases, heavy_after));
}

pub fn outcome_text(p: &Plan, cases: &[Case], heavy_after: f64) -> String {
    let mut t = String::new();
    if cases.is_empty() {
        return t;
    }
    let mut ran: Vec<&Case> = cases
        .iter()
        .filter(|c| c.outcome != Outcome::Skipped)
        .collect();
    ran.sort_by(|a, b| b.secs.total_cmp(&a.secs));
    let total: f64 = ran.iter().map(|c| c.secs).sum();
    let heavy = ran.iter().filter(|c| c.secs >= heavy_after).count();
    let _ = writeln!(
        t,
        "\nmock test: {} test(s) ran, {:.0}s of test time, {heavy} at or over {heavy_after}s; slowest:",
        ran.len(),
        total
    );
    for c in ran.iter().take(SLOWEST) {
        let mark = match c.outcome {
            Outcome::Fail => "  FAILED",
            _ => "",
        };
        let _ = writeln!(t, "    {:>8.1}s  {} {}{mark}", c.secs, c.binary, c.name);
    }
    let skipped = p.skip.len();
    if skipped > 0 {
        let _ = writeln!(
            t,
            "mock test: {skipped} heavy test(s) not run, listed above"
        );
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::select::Skip;

    fn inv(args: &[&str]) -> Invocation {
        crate::suite::args::parse(args)
    }

    #[test]
    fn the_plan_says_why_each_member_runs() {
        let p = Plan {
            run:     vec![
                ("mesh".to_string(), Why::Moved(vec!["mesh".to_string()])),
                (
                    "web".to_string(),
                    Why::Moved(vec!["mesh".to_string(), "render".to_string()]),
                ),
                ("new".to_string(), Why::NeverGreen),
            ],
            settled: vec!["tool".to_string()],
            heavy:   vec![("render".to_string(), "drawn".to_string())],
            skip:    vec![Skip {
                binary: "render".to_string(),
                name:   "drawn".to_string(),
                why:    SkipWhy::Cached,
            }],
        };
        let t = plan_text(
            &p,
            &inv(&["--features", "editor"]),
            &Fingerprints::default(),
            10.0,
            1,
        );
        assert!(
            t.contains("nextest, what changed, --features editor ==="),
            "{t}"
        );
        assert!(t.contains("run      mesh: its own inputs moved"), "{t}");
        assert!(t.contains("run      web: moved: mesh, render"), "{t}");
        assert!(t.contains("run      new: no earlier pass on record"), "{t}");
        assert!(
            t.contains("settled  1 member(s) passed at what they are now: tool"),
            "{t}"
        );
        assert!(
            t.contains("heavy    1 test(s) took 10s or more last time; 1 at a time"),
            "{t}"
        );
        assert!(t.contains("cached   1 heavy test(s) not run"), "{t}");
        assert!(t.contains("render drawn"), "{t}");
    }

    #[test]
    fn the_outcome_names_the_slowest_first_and_marks_failures() {
        let case = |name: &str, secs: f64, outcome| {
            Case {
                binary: "render".to_string(),
                name: name.to_string(),
                secs,
                outcome,
            }
        };
        let cases = vec![
            case("quick", 0.1, Outcome::Pass),
            case("drawn", 160.0, Outcome::Pass),
            case("broken", 12.0, Outcome::Fail),
            case("ignored", 0.0, Outcome::Skipped),
        ];
        let t = outcome_text(&Plan::default(), &cases, 10.0);
        assert!(
            t.contains("3 test(s) ran, 172s of test time, 2 at or over 10s"),
            "{t}"
        );
        let drawn = t.find("render drawn").unwrap();
        let broken = t.find("render broken  FAILED").unwrap();
        let quick = t.find("render quick").unwrap();
        assert!(drawn < broken && broken < quick, "{t}");
        assert!(!t.contains("ignored"), "{t}");
    }
}
