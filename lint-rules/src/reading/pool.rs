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
//! while some of its own are still being judged or a thread it started has not yet ended. So a
//! thread inside a unit helps the `each` calls of other units, and is never caught judging a unit
//! of the run around it while its own call has not returned.

use std::any::Any;
use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// A batch's work, with the lifetime of the call that opened it taken off. It is called only while
/// the batch is on the open list, and the call that opened it does not return before the batch is
/// done and taken off the list, so the closure is alive whenever it is called. That holds when the
/// call unwinds too: the call's `Close` guard stops the batch handing out items, waits for every
/// item already handed out to return, and takes the batch off the list before the closure drops.
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

/// Takes a batch off the open list however the call that opened it ends. On the way out of a panic
/// it first stops the batch handing out items and waits for those being worked to return, so the
/// closure the list points at is never called once the call has returned.
struct Close<'b> {
    budget: &'b Budget,
    id:     u64,
}

impl Drop for Close<'_> {
    fn drop(&mut self) {
        let mut open = self.budget.lock();
        loop {
            let Some(at) = open.open.iter().position(|b| b.id == self.id) else { return };
            let batch = &mut open.open[at];
            batch.n = batch.next;
            if batch.done == batch.next {
                open.open.remove(at);
                self.budget.wake.notify_all();
                return;
            }
            open = self.budget.wake.wait(open).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// `work(i)` for every `i` in `order`, handed out in that order to the asking thread, to as many
/// spare threads of `budget` as there is work for, and to any thread of the run that runs out of
/// its own, each result at position `i` of what comes back. `order` names each of `0 .. n` once.
/// With no other thread free, the work runs on the asking thread, one item after another in
/// `order`.
///
/// The asking thread returns once every item has returned and every thread it started has found
/// nothing left to take; until then it takes work as they do, so it never sits waiting on a thread
/// that is doing what it could be doing.
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
    // Threads started for this call and not yet ended.
    let live = AtomicUsize::new(0);

    let panic = std::thread::scope(|scope| {
        let _close = Close { budget, id };
        for _ in 0 .. budget.take(n - 1) {
            live.fetch_add(1, Ordering::AcqRel);
            let worker = std::thread::Builder::new().spawn_scoped(scope, || {
                loop {
                    let claim = Budget::claim(&mut budget.lock(), id, depth);
                    let Some(claim) = claim else { break };
                    budget.work(claim);
                }
                budget.give(1);
                let _open = budget.lock();
                live.fetch_sub(1, Ordering::AcqRel);
                budget.wake.notify_all();
            });
            // A thread the system will not start is given back, and the others, the asking one
            // among them, do its share.
            if worker.is_err() {
                live.fetch_sub(1, Ordering::AcqRel);
                budget.give(1);
            }
        }
        let mut open = budget.lock();
        let mut panic = None;
        let mut finished = false;
        loop {
            if let Some(claim) = Budget::claim(&mut open, id, depth) {
                drop(open);
                budget.work(claim);
                #[cfg(test)]
                tests::fail_point();
                open = budget.lock();
                continue;
            }
            if !finished {
                let at = open.open.iter().position(|b| b.id == id).expect("this batch is open");
                if open.open[at].done == n {
                    panic = open.open.remove(at).panic;
                    finished = true;
                }
            }
            if finished && live.load(Ordering::Acquire) == 0 {
                break panic;
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
        let released = AtomicUsize::new(0);
        // Each item holds its thread until four are at work at once, after which none waits, so a
        // fifth thread at work would be seen.
        let busy = || {
            let now = working.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            until(|| released.load(Ordering::SeqCst) == 1 || working.load(Ordering::SeqCst) >= 4);
            released.store(1, Ordering::SeqCst);
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
        assert_eq!(most, 4, "every thread of the budget and no more worked at once");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 3, "every thread was given back");
    }

    /// A unit whose own items are all handed out, waiting on the last of them, takes the items of
    /// another unit's `each` opened before its own, rather than sit idle beside them.
    #[test]
    fn a_unit_waiting_on_its_last_item_takes_another_units_items() {
        let budget = Budget::new(4);
        let helped = AtomicUsize::new(0);
        let second = Mutex::new(None);
        let open_of = |n: usize| budget.lock().open.iter().any(|b| b.n == n);
        let handed_out = |n: usize| budget.lock().open.iter().any(|b| b.n == n && b.next == n);
        each_index(&budget, 3, |unit| match unit {
            // A unit splitting into many items, each held until the second unit's thread has
            // taken one of them, so the threads already on them cannot finish the split alone.
            0 => {
                each_index(&budget, 40, |_| {
                    if *second.lock().unwrap() == Some(std::thread::current().id()) {
                        helped.fetch_add(1, Ordering::SeqCst);
                    }
                    until(|| helped.load(Ordering::SeqCst) > 0);
                });
            },
            // A unit splitting, once the first has split and taken the last spare thread, into two
            // items. Its own thread returns from whichever it takes once both are handed out; the
            // free thread taking the other holds it until the first unit was helped, so the second
            // unit waits on it with nothing of its own left.
            1 => {
                assert!(until(|| open_of(40) && budget.spare.load(Ordering::SeqCst) == 0), "the first unit split");
                let me = std::thread::current().id();
                *second.lock().unwrap() = Some(me);
                each_index(&budget, 2, |_| {
                    if std::thread::current().id() == me {
                        assert!(until(|| handed_out(2)), "the other item was taken");
                    } else {
                        until(|| helped.load(Ordering::SeqCst) > 0);
                    }
                });
            },
            // A unit done once the second has split, whose thread is then free to take its item.
            _ => assert!(until(|| open_of(2)), "the second unit split"),
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

    std::thread_local! {
        /// Set by a test to make the asking thread panic after the next item it works, outside the
        /// item, the way a fault in the pool itself would unwind out of [`in_order`].
        static FAIL: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn fail_point() {
        if FAIL.replace(false) {
            panic!("the asking thread fails between items");
        }
    }

    #[test]
    fn a_call_unwinding_between_items_takes_its_batch_off_the_list() {
        let budget = Budget::new(1);
        let started = AtomicUsize::new(0);
        FAIL.set(true);
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
            each_index(&budget, 4, |_| {
                started.fetch_add(1, Ordering::SeqCst);
            })
        }));
        assert!(caught.is_err(), "the unwind reached the caller");
        assert!(budget.lock().open.is_empty(), "no batch is left whose work has gone");
        assert_eq!(started.load(Ordering::SeqCst), 1, "no item was handed out after the unwind");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 0, "the budget is as it was");
        assert_eq!(each_index(&budget, 3, |i| i), [0, 1, 2], "the budget still works");
    }

    /// Whether `ready` comes true within five seconds, asked over and over in the meantime.
    fn until(ready: impl Fn() -> bool) -> bool {
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < end {
            if ready() {
                return true;
            }
            std::thread::yield_now();
        }
        ready()
    }

    #[test]
    fn a_call_whose_items_are_done_works_on_while_its_threads_do() {
        // The asking thread of `a` and the one thread it starts each take one of `a`'s items. The
        // started one then takes the second item of another call, `b`, and that item splits itself
        // only once `a`'s batch is done. The asking thread of `a` has to take the split's second
        // item, since every other thread is busy, or the split never has two items in flight.
        let budget = Budget::new(2);
        let a_in = AtomicUsize::new(0);
        let in_flight = AtomicUsize::new(0);
        let reached = AtomicUsize::new(0);
        let split_done = AtomicUsize::new(0);
        let only_open = |n: usize| until(|| budget.lock().open.len() == n);
        std::thread::scope(|scope| {
            let asking = scope.spawn(|| {
                let me = std::thread::current().id();
                each_index(&budget, 2, |_| {
                    if std::thread::current().id() == me {
                        a_in.store(1, Ordering::SeqCst);
                        assert!(
                            until(|| budget.lock().open.iter().any(|b| b.depth == 0 && b.n == 2 && b.next == 2 && b.id != 0)),
                            "the other call's items were all handed out"
                        );
                    } else {
                        // Waiting for the other call to open keeps this thread from finding nothing
                        // to take and ending before there is anything of `b` to take.
                        assert!(until(|| budget.lock().open.iter().any(|b| b.id != 0)), "the other call opened");
                    }
                })
            });
            assert!(until(|| a_in.load(Ordering::SeqCst) == 1), "the call opened");
            each_index(&budget, 2, |i| {
                if i == 0 {
                    assert!(until(|| split_done.load(Ordering::SeqCst) == 1), "the split ended");
                    return;
                }
                assert!(only_open(1), "the first call's batch was taken off the list");
                each_index(&budget, 2, |_| {
                    in_flight.fetch_add(1, Ordering::SeqCst);
                    if until(|| in_flight.load(Ordering::SeqCst) >= 2) {
                        reached.store(1, Ordering::SeqCst);
                    }
                });
                split_done.store(1, Ordering::SeqCst);
            });
            asking.join().expect("the first call returned");
        });
        assert_eq!(reached.load(Ordering::SeqCst), 1, "both items of the split were in flight at once");
        assert!(budget.lock().open.is_empty(), "every batch was taken off the list");
        assert_eq!(budget.spare.load(Ordering::SeqCst), 1, "every thread was given back");
    }
}
