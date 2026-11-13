//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Work spread over the run's threads, with each result put back where its input was.
//!
//! A run has one budget of threads, its reading's, and one list of the work open on them. Asking
//! for work done opens a batch of items on the list. The asking thread is one of the run's and
//! works too; more are started while the budget has any spare, and each is given back the moment
//! it finds nothing left to take. A thread with nothing of its own takes the next item not yet
//! started of the newest open batch, so work asked for from inside work, a lint splitting itself
//! with `View::each` while the run's units are judged, is taken up by every thread that finishes
//! its unit, and a thread that started it alone does not finish it alone. The run never has more
//! threads working than its reading names.
//!
//! A thread takes the items of its own batch first. Once those are all handed out it takes items
//! of any batch asked for as deep in work as its own or deeper, and waits only when there are none
//! while some of its own are still being judged. So a thread inside a unit helps the `each` calls
//! of other units, and is never caught judging a unit of the run around it while its own call has
//! not returned.

use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// A batch's work, with the lifetime of the call that opened it taken off. It is called only while
/// the batch is on the open list, and the call that opened it does not return before the batch is
/// done and taken off the list, so the closure is alive whenever it is called.
type Job = *const (dyn Fn(usize) + Sync);

/// One batch on the open list.
struct Open {
    id:    u64,
    /// How deep in work this batch was asked for: 0 for a run's units, 1 for an `each` inside one.
    depth: usize,
    n:     usize,
    /// The next item to hand out.
    next:  usize,
    /// Items whose work has returned.
    done:  usize,
    job:   Job,
    /// The first panic of an item, raised again on the asking thread.
    panic: Option<Box<dyn Any + Send>>,
}

// SAFETY: `Open` crosses threads only inside the list's mutex, and `job` points at a closure that
// is `Sync` and alive while the batch is open, as `Job` says.
unsafe impl Send for Open {}

#[derive(Default)]
struct Batches {
    ids:  u64,
    open: Vec<Open>,
}

/// The threads a run may have working beside the ones already working, and the work open on them.
pub(crate) struct Budget {
    spare:   AtomicUsize,
    batches: Mutex<Batches>,
    /// Woken when a batch opens or one is done.
    wake:    Condvar,
}

thread_local! {
    /// How deep in work this thread is: 0 outside any item, one more inside each item it is doing.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// An item handed out: its batch, its position in the batch's order, and the batch's work.
struct Claim {
    id:    u64,
    depth: usize,
    at:    usize,
    job:   Job,
}

impl Budget {
    /// A budget for `threads` threads in all, the asking one among them.
    pub(crate) fn new(threads: usize) -> Self {
        Self {
            spare:   AtomicUsize::new(threads.max(1) - 1),
            batches: Mutex::default(),
            wake:    Condvar::new(),
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

    fn lock(&self) -> MutexGuard<'_, Batches> {
        self.batches.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The next item not yet handed out of batch `own`, else of the newest open batch at depth
    /// `floor` or deeper.
    fn claim(open: &mut Batches, own: u64, floor: usize) -> Option<Claim> {
        let batch = match open.open.iter().rposition(|b| b.id == own && b.next < b.n) {
            Some(at) => &mut open.open[at],
            None => open.open.iter_mut().rev().find(|b| b.depth >= floor && b.next < b.n)?,
        };
        let at = batch.next;
        batch.next += 1;
        Some(Claim {
            id: batch.id,
            depth: batch.depth,
            at,
            job: batch.job,
        })
    }

    /// Do one handed-out item and count it done, keeping a panic for the asking thread.
    fn work(&self, claim: Claim) {
        // SAFETY: the batch is open until its last item is counted done below, and the call that
        // opened it keeps the closure alive until then.
        let job = unsafe { &*claim.job };
        let outer = DEPTH.replace(claim.depth + 1);
        let result = panic::catch_unwind(AssertUnwindSafe(|| job(claim.at)));
        DEPTH.set(outer);
        let mut open = self.lock();
        let batch = open
            .open
            .iter_mut()
            .find(|b| b.id == claim.id)
            .expect("a batch stays open until its items are done");
        batch.done += 1;
        if let Err(payload) = result {
            batch.panic.get_or_insert(payload);
        }
        if batch.done == batch.n {
            self.wake.notify_all();
        }
    }
}

/// `work(i)` for every `i` in `order`, handed out in that order to the asking thread, to as many
/// spare threads of `budget` as there is work for, and to any thread of the run that runs out of
/// its own, each result at position `i` of what comes back. `order` names each of `0 .. n` once.
/// With no other thread free, the work runs on the asking thread, one item after another in
/// `order`.
///
/// # Panics
///
/// With the first panic of any item, on the asking thread, once every item has returned.
pub(crate) fn in_order<T: Send>(budget: &Budget, n: usize, order: &[usize], work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    debug_assert_eq!(order.len(), n, "the order names every item once");
    if n == 0 {
        return Vec::new();
    }
    let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(n));
    let run = |at: usize| {
        let i = order[at];
        let out = work(i);
        done.lock().unwrap_or_else(PoisonError::into_inner).push((i, out));
    };
    let job: &(dyn Fn(usize) + Sync) = &run;
    // SAFETY: only the lifetime is changed, and `Job` says why the closure outlives every call.
    let job: &'static (dyn Fn(usize) + Sync) = unsafe { std::mem::transmute(job) };
    let depth = DEPTH.get();
    let id = {
        let mut open = budget.lock();
        let id = open.ids;
        open.ids += 1;
        open.open.push(Open {
            id,
            depth,
            n,
            next: 0,
            done: 0,
            job: job as Job,
            panic: None,
        });
        id
    };
    budget.wake.notify_all();

    let panic = std::thread::scope(|scope| {
        for _ in 0 .. budget.take(n - 1) {
            let worker = std::thread::Builder::new().spawn_scoped(scope, || {
                loop {
                    let claim = Budget::claim(&mut budget.lock(), id, depth);
                    let Some(claim) = claim else { break };
                    budget.work(claim);
                }
                budget.give(1);
            });
            // A thread the system will not start is given back, and the others, the asking one
            // among them, do its share.
            if worker.is_err() {
                budget.give(1);
            }
        }
        let mut open = budget.lock();
        loop {
            if let Some(claim) = Budget::claim(&mut open, id, depth) {
                drop(open);
                budget.work(claim);
                open = budget.lock();
                continue;
            }
            let at = open.open.iter().position(|b| b.id == id).expect("this batch is open");
            if open.open[at].done == n {
                break open.open.remove(at).panic;
            }
            open = budget.wake.wait(open).unwrap_or_else(PoisonError::into_inner);
        }
    });
    if let Some(payload) = panic {
        panic::resume_unwind(payload);
    }
    let mut slots: Vec<Option<T>> = (0 .. n).map(|_| None).collect();
    for (i, out) in done.into_inner().unwrap_or_else(PoisonError::into_inner) {
        slots[i] = Some(out);
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

    /// A unit whose own items are all handed out, waiting on the last of them, takes the items of
    /// another unit's `each` opened before its own, rather than sit idle beside them.
    #[test]
    fn a_unit_waiting_on_its_last_item_takes_another_units_items() {
        let budget = Budget::new(4);
        let first_open = AtomicUsize::new(0);
        let second_open = AtomicUsize::new(0);
        let helped = AtomicUsize::new(0);
        let second = Mutex::new(None);
        each_index(&budget, 3, |unit| match unit {
            // A unit splitting into many short items.
            0 => {
                first_open.store(1, Ordering::SeqCst);
                each_index(&budget, 40, |_| {
                    std::thread::sleep(Duration::from_millis(5));
                    let here = Some(std::thread::current().id());
                    if second_open.load(Ordering::SeqCst) == 1 && *second.lock().unwrap() == here {
                        helped.fetch_add(1, Ordering::SeqCst);
                    }
                });
            },
            // A unit splitting later into one short item it takes itself and one long item a
            // free thread takes, so it waits on that one with nothing of its own left.
            1 => {
                while first_open.load(Ordering::SeqCst) == 0 {
                    std::thread::yield_now();
                }
                std::thread::sleep(Duration::from_millis(2));
                *second.lock().unwrap() = Some(std::thread::current().id());
                second_open.store(1, Ordering::SeqCst);
                each_index(&budget, 2, |i| std::thread::sleep(Duration::from_millis(if i == 0 { 10 } else { 80 })));
                second_open.store(0, Ordering::SeqCst);
            },
            // A unit done at once, whose thread is then free.
            _ => std::thread::sleep(Duration::from_millis(1)),
        });
        let helped = helped.load(Ordering::SeqCst);
        assert!(helped > 0, "the waiting unit took none of the other unit's items");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 3, "every thread was given back");
    }

    /// An item that panics is raised on the asking thread once every other item has returned, and
    /// the budget is left as it was, so the run's other work goes on.
    #[test]
    fn a_panicking_item_reaches_the_asking_thread_after_the_rest() {
        let budget = Budget::new(4);
        let finished = AtomicUsize::new(0);
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
            each_index(&budget, 16, |i| {
                std::thread::sleep(Duration::from_millis(2));
                assert!(i != 3, "item three fails");
                finished.fetch_add(1, Ordering::SeqCst);
            })
        }));
        assert!(caught.is_err(), "the panic reached the asking thread");
        assert_eq!(finished.load(Ordering::SeqCst), 15, "every other item ran to its end first");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 3, "every thread was given back");
        assert!(budget.lock().open.is_empty(), "the batch was taken off the list");
        assert_eq!(each_index(&budget, 3, |i| i), [0, 1, 2], "the budget still works");
    }
}
