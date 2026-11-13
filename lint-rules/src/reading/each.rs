//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A lint that spends its time on separable items, each judged with its own view and trace.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::store::Alt;
use super::{Trace, UnitKey, View, pool};
use crate::LintError;

/// What names an item of [`View::each`] from one run to the next.
///
/// Two items of one call with the same key share a saved verdict, so the key says which item it
/// is, a path or a name, and not where in the list it stood.
pub trait Item {
    /// The item's name, stable across runs.
    fn key(&self) -> Cow<'_, str>;
}

impl Item for str {
    fn key(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl Item for String {
    fn key(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl Item for Path {
    fn key(&self) -> Cow<'_, str> {
        crate::reading::names::text_of(self.as_os_str())
    }
}

impl Item for PathBuf {
    fn key(&self) -> Cow<'_, str> {
        crate::reading::names::text_of(self.as_os_str())
    }
}

impl<T: Item + ?Sized> Item for &T {
    fn key(&self) -> Cow<'_, str> {
        (**self).key()
    }
}

/// What one item came to: its findings, its trace, and what to save for it.
struct Judged {
    findings: Vec<LintError>,
    trace:    Trace,
    save:     Option<(String, Vec<Alt>, std::time::Duration)>,
}

impl<'r> View<'r> {
    /// Judge every item with a view and a trace of its own, on the machine's threads, and give
    /// back each item's findings in the order of `items`.
    ///
    /// This replaces a hand-written list of what each item depends on. An item whose trace still
    /// holds is replayed and not judged; the others are judged at the same time, so `judge` runs
    /// on worker threads and holds no state it changes. `judge` asks everything through the view it
    /// is handed and never through this one: a question put to this view while items are being
    /// judged is not in any item's trace, so the items of this call are then saved for nobody and
    /// judged again on the next run. An `each` asked of this view from inside an item is such a
    /// question too, and nothing it does clears the mark.
    ///
    /// Everything the items asked is written into this view's trace too, so a unit whose items did
    /// not move is replayed whole without being split again.
    ///
    /// An item is saved under the unit, the place in the source this call is made from (its file,
    /// line and column), how many calls the unit made from that place before this one, and
    /// [`Item::key`], and under nothing else. So a call skipped on one run does not hand its saved
    /// items to another call. Whatever `judge` takes from outside the item and the view, a table it
    /// was built with, a setting, a value worked out before the call, has to be in the item's key
    /// too, or an item judged under one such value is replayed under another.
    ///
    /// The place is the caller's, found through `#[track_caller]`. A helper function that calls
    /// `each` for its callers is that place for all of them, unless it is `#[track_caller]` itself,
    /// and its calls are then told apart only by their count: one call of the helper skipped on a
    /// run hands its items to the next call of the helper. Mark such a helper `#[track_caller]`, or
    /// put what tells its calls apart into the item key. A loop calling `each` from one place is
    /// the same, and its items carry what differs between the turns of the loop.
    ///
    /// The items are handed out in their order to the calling thread, to the run's threads that
    /// are free, and to every thread of the run that finishes its own work while some are not yet
    /// started, so a unit splitting itself while other units are judged gets the threads they
    /// leave and never puts more threads to work than the run has.
    #[track_caller]
    pub fn each<T, F>(&self, items: &[T], judge: F) -> Vec<Vec<LintError>>
    where
        T: Item + Sync,
        F: Fn(&View<'r>, &T) -> Vec<LintError> + Sync,
    {
        let reading = self.reading();
        let place = std::panic::Location::caller();
        let call = {
            let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            let n = calls.entry((place.file(), place.line(), place.column())).or_insert(0);
            *n += 1;
            format!("{}:{}:{}#{}", place.file(), place.line(), place.column(), *n - 1)
        };
        // An `each` asked of this view while another is judging items is a question the outer
        // items do not hold, as any other question to this view then is.
        if self.busy.load(Ordering::Acquire) > 0 {
            self.stray.fetch_add(1, Ordering::AcqRel);
        }
        let strays = self.stray.load(Ordering::Acquire);
        self.busy.fetch_add(1, Ordering::AcqRel);
        let judged: Vec<Judged> = pool::each_index(reading.pool(), items.len(), |i| {
            let item = &items[i];
            let unit = self.unit.as_ref().map(|u| {
                let key = u
                    .store
                    .unit_key(&[u.key.as_bytes(), b"item", call.as_bytes(), item.key().as_bytes()]);
                UnitKey {
                    store: u.store,
                    key,
                    lint: u.lint,
                }
            });
            let mut prior = Vec::new();
            if let Some(u) = &unit {
                let found = u.store.lookup(&u.key);
                u.store.cache().tally(found.hit.is_some(), u.lint);
                if let Some((findings, trace)) = found.hit {
                    return Judged {
                        findings,
                        trace,
                        save: None,
                    };
                }
                prior = found.prior;
            }
            let view = View::new(reading, unit.clone());
            let start = Instant::now();
            let findings = judge(&view, item);
            let took = start.elapsed();
            Judged {
                findings,
                trace: view.into_trace(),
                save: unit.map(|u| (u.key, prior, took)),
            }
        });
        self.busy.fetch_sub(1, Ordering::AcqRel);
        let stray = self.stray.load(Ordering::Acquire) != strays;
        let mut out = Vec::with_capacity(judged.len());
        for j in judged {
            if let (false, Some((key, prior, took)), Some(u)) = (stray, &j.save, &self.unit) {
                u.store.save(key, prior.clone(), &j.trace, &j.findings, *took);
            }
            self.absorb(&j.trace);
            out.push(j.findings);
        }
        out
    }
}
