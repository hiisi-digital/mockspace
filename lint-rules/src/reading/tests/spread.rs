//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A unit splitting itself with `each` while the run's other units are being judged gets every
//! thread that has nothing else to do, and the run never has more threads at work than it names.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::*;

/// What every unit and item of one run counts, shared by the two lints.
#[derive(Default)]
struct Counts {
    /// Threads judging a unit or an item at this moment.
    working:     AtomicUsize,
    most:        AtomicUsize,
    /// Items of the split unit being judged at this moment.
    items:       AtomicUsize,
    most_items:  AtomicUsize,
    /// Small units still to finish.
    small_left:  AtomicUsize,
    /// The most items judged at once after the last small unit finished.
    most_after:  AtomicUsize,
    /// The sum of every item's cost, in microseconds.
    item_micros: AtomicUsize,
}

impl Counts {
    fn enter(&self) {
        let now = self.working.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.working.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A crate lint over many crates, each a short unit.
struct Small(Arc<Counts>);

impl Lint for Small {
    fn name(&self) -> &'static str {
        "small"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn per_file(&self) -> bool {
        false
    }
}

impl crate::CrateLint for Small {
    fn check(&self, _ctx: &crate::LintContext) -> Vec<LintError> {
        self.0.enter();
        std::thread::sleep(Duration::from_millis(4));
        self.0.leave();
        self.0.small_left.fetch_sub(1, Ordering::SeqCst);
        Vec::new()
    }
}

/// A repo lint splitting itself into items of uneven cost.
struct Split(Arc<Counts>, usize);

impl Lint for Split {
    fn name(&self) -> &'static str {
        "split"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for Split {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let items: Vec<String> = (0 .. self.1).map(|i| format!("item-{i:03}")).collect();
        ctx.view()
            .each(&items, |_, item| {
                let c = &self.0;
                c.enter();
                let now = c.items.fetch_add(1, Ordering::SeqCst) + 1;
                c.most_items.fetch_max(now, Ordering::SeqCst);
                let n: usize = item[5 ..].parse().unwrap();
                let cost = Duration::from_millis(4 + (n as u64 * 7) % 23);
                std::thread::sleep(cost);
                if c.small_left.load(Ordering::SeqCst) == 0 {
                    c.most_after.fetch_max(c.items.load(Ordering::SeqCst), Ordering::SeqCst);
                }
                c.item_micros.fetch_add(cost.as_micros() as usize, Ordering::SeqCst);
                c.items.fetch_sub(1, Ordering::SeqCst);
                c.leave();
                vec![LintError::error("workspace".into(), 1, "split", item.clone())]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// One run of `crates` small units beside one unit of `items` items, on `threads` threads, with
/// no store. Returns the counts, the run's wall time and the split unit's findings in order.
fn run_spread(crates: usize, items: usize, threads: usize) -> (Arc<Counts>, Duration, Vec<String>) {
    let names: Vec<String> = (0 .. crates).map(|i| format!("c{i:03}")).collect();
    let files: &[(&str, &str)] = &[("src/lib.rs", "pub fn f() {}\n")];
    let list: Vec<(&str, &[(&str, &str)])> = names.iter().map(|n| (n.as_str(), files)).collect();
    let w = Workspace::new(&format!("spread-{threads}"), &list);
    let counts = Arc::new(Counts::default());
    counts.small_left.store(crates, Ordering::SeqCst);
    let pack = LintPack {
        crate_lints: vec![Box::new(Small(Arc::clone(&counts)))],
        repo_lints: vec![Box::new(Split(Arc::clone(&counts), items))],
        ..LintPack::default()
    };
    let reading = Reading::new(&w.dir.0).with_threads(threads);
    let start = Instant::now();
    let found = w.run_with(&reading, None, None, Some(&pack));
    let wall = start.elapsed();
    let split = found.iter().filter(|e| e.lint_name == "split").map(|e| e.message.clone()).collect();
    (counts, wall, split)
}

/// Once the small units are done, every one of the run's threads judges the split unit's items,
/// and at no moment are more threads at work than the run names. The items come back in order.
#[test]
fn idle_threads_take_the_items_of_a_unit_already_splitting() {
    let (counts, _, split) = run_spread(48, 64, 4);
    let want: Vec<String> = (0 .. 64).map(|i| format!("item-{i:03}")).collect();
    assert_eq!(split, want, "the findings come back in the items' order");
    let most = counts.most.load(Ordering::SeqCst);
    assert!(most <= 4, "{most} threads were at work at once on a run of four");
    let after = counts.most_after.load(Ordering::SeqCst);
    assert_eq!(after, 4, "once the small units were done, only {after} of four threads judged items");
}

/// The measurement behind the test above: the run's wall time against the sum of the split
/// unit's item costs, at one thread and at four. Prints, asserts nothing.
#[test]
#[ignore = "a measurement, run by hand with --ignored --nocapture"]
fn spread_wall_against_item_cost() {
    for threads in [1, 4] {
        let (counts, wall, _) = run_spread(400, 160, threads);
        let items = Duration::from_micros(counts.item_micros.load(Ordering::SeqCst) as u64);
        eprintln!(
            "threads {threads}: wall {wall:?}, item cost {items:?}, most at work {}, most items at once {}, after small units {}",
            counts.most.load(Ordering::SeqCst),
            counts.most_items.load(Ordering::SeqCst),
            counts.most_after.load(Ordering::SeqCst),
        );
    }
}
