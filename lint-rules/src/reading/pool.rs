//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Work spread over the run's threads, with each result put back where its input was.
//!
//! A run has one budget of threads, its reading's. The thread that asks for work done is one of
//! them and works too; the others are taken from what the budget has spare, and each is given back
//! the moment it finds nothing left to take. So work asked for from inside work, a lint splitting
//! itself with `View::each` while the run's units are being judged, takes only threads nobody is
//! using, and runs on its own thread when none are spare. The run never has more threads working
//! than its reading names.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// The threads a run may have working beside the ones already working.
pub(crate) struct Budget {
    spare: AtomicUsize,
}

impl Budget {
    /// A budget for `threads` threads in all, the asking one among them.
    pub(crate) fn new(threads: usize) -> Self {
        Self {
            spare: AtomicUsize::new(threads.max(1) - 1),
        }
    }

    /// Up to `want` threads, as many as are spare.
    fn take(&self, want: usize) -> usize {
        let mut now = self.spare.load(Ordering::Acquire);
        loop {
            let got = now.min(want);
            if got == 0 {
                return 0;
            }
            match self.spare.compare_exchange_weak(now, now - got, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return got,
                Err(seen) => now = seen,
            }
        }
    }

    fn give(&self, n: usize) {
        self.spare.fetch_add(n, Ordering::AcqRel);
    }
}

/// `work(i)` for every `i` in `order`, taken in that order by the asking thread and by as many
/// spare threads of `budget` as there is work for, each result at position `i` of what comes back.
/// `order` names each of `0 .. n` once. With no thread spare, the work runs on the asking thread,
/// one item after another in `order`.
pub(crate) fn in_order<T: Send>(budget: &Budget, n: usize, order: &[usize], work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    debug_assert_eq!(order.len(), n, "the order names every item once");
    let mut slots: Vec<Option<T>> = (0 .. n).map(|_| None).collect();
    let extra = budget.take(n.saturating_sub(1));
    if extra == 0 {
        for &i in order {
            slots[i] = Some(work(i));
        }
    } else {
        let next = AtomicUsize::new(0);
        let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(n));
        let drain = || {
            loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = order.get(at) else { break };
                let out = work(i);
                done.lock().unwrap_or_else(PoisonError::into_inner).push((i, out));
            }
        };
        std::thread::scope(|scope| {
            for _ in 0 .. extra {
                let worker = std::thread::Builder::new().spawn_scoped(scope, || {
                    drain();
                    budget.give(1);
                });
                // A thread the system will not start is given back, and the others, the asking
                // one among them, do its share.
                if worker.is_err() {
                    budget.give(1);
                }
            }
            drain();
        });
        for (i, out) in done.into_inner().unwrap_or_else(PoisonError::into_inner) {
            slots[i] = Some(out);
        }
    }
    slots
        .into_iter()
        .map(|s| s.expect("every position in the order was worked"))
        .collect()
}

/// [`in_order`] over `0 .. n` in its own order.
pub(crate) fn each_index<T: Send>(budget: &Budget, n: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let order: Vec<usize> = (0 .. n).collect();
    in_order(budget, n, &order, work)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn one_thread_works_in_the_order_given_and_many_put_results_back_in_place() {
        let seen = Mutex::new(Vec::new());
        let out = in_order(&Budget::new(1), 4, &[2, 0, 3, 1], |i| {
            seen.lock().unwrap().push(i);
            i * 10
        });
        assert_eq!(out, [0, 10, 20, 30]);
        assert_eq!(*seen.lock().unwrap(), [2, 0, 3, 1], "one thread runs in the order named");

        let out = in_order(&Budget::new(4), 50, &(0 .. 50).rev().collect::<Vec<_>>(), |i| i + 1);
        assert_eq!(out, (1 ..= 50).collect::<Vec<_>>());
    }

    /// Work asked for from inside work shares the one budget, so the run never has more threads
    /// working than it names, and the inner results still come back in their order.
    #[test]
    fn work_inside_work_takes_only_spare_threads() {
        let budget = Budget::new(4);
        let working = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let busy = || {
            let now = working.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5));
            working.fetch_sub(1, Ordering::SeqCst);
        };
        let out = each_index(&budget, 8, |outer| {
            let inner = each_index(&budget, 8, |i| {
                busy();
                outer * 100 + i
            });
            assert_eq!(inner, (0 .. 8).map(|i| outer * 100 + i).collect::<Vec<_>>());
            outer
        });
        assert_eq!(out, (0 .. 8).collect::<Vec<_>>());
        let most = most.load(Ordering::SeqCst);
        assert!(most <= 4, "{most} threads worked at once on a budget of 4");
        assert!(most >= 2, "the control: the budget was used at all ({most})");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 3, "every thread was given back");
    }
}
