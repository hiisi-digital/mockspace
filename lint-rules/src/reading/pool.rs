//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Work spread over the machine's threads, with each result put back where its input was.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// `work(i)` for every `i` in `order`, taken in that order by up to `threads` threads, each result
/// at position `i` of what comes back. `order` names each of `0 .. n` once. On one thread the work
/// runs on the caller's, one item after another in `order`.
pub(crate) fn in_order<T: Send>(threads: usize, n: usize, order: &[usize], work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    debug_assert_eq!(order.len(), n, "the order names every item once");
    let workers = threads.min(n).max(1);
    let mut slots: Vec<Option<T>> = (0 .. n).map(|_| None).collect();
    if workers == 1 {
        for &i in order {
            slots[i] = Some(work(i));
        }
    } else {
        let next = AtomicUsize::new(0);
        let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(n));
        std::thread::scope(|scope| {
            for _ in 0 .. workers {
                scope.spawn(|| {
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&i) = order.get(at) else { break };
                        let out = work(i);
                        done.lock().unwrap_or_else(PoisonError::into_inner).push((i, out));
                    }
                });
            }
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
pub(crate) fn each_index<T: Send>(threads: usize, n: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let order: Vec<usize> = (0 .. n).collect();
    in_order(threads, n, &order, work)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_thread_works_in_the_order_given_and_many_put_results_back_in_place() {
        let seen = Mutex::new(Vec::new());
        let out = in_order(1, 4, &[2, 0, 3, 1], |i| {
            seen.lock().unwrap().push(i);
            i * 10
        });
        assert_eq!(out, [0, 10, 20, 30]);
        assert_eq!(*seen.lock().unwrap(), [2, 0, 3, 1], "one thread runs in the order named");

        let out = in_order(4, 50, &(0 .. 50).rev().collect::<Vec<_>>(), |i| i + 1);
        assert_eq!(out, (1 ..= 50).collect::<Vec<_>>());
    }
}
