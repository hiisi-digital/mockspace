//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A lint that spends its time on separable items, each judged with its own view and trace.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
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
        self.to_string_lossy()
    }
}

impl Item for PathBuf {
    fn key(&self) -> Cow<'_, str> {
        self.to_string_lossy()
    }
}

impl<T: Item + ?Sized> Item for &T {
    fn key(&self) -> Cow<'_, str> {
        (**self).key()
    }
}

impl<K: Item, V> Item for (K, V) {
    fn key(&self) -> Cow<'_, str> {
        self.0.key()
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
    /// judged again on the next run.
    ///
    /// Everything the items asked is written into this view's trace too, so a unit whose items did
    /// not move is replayed whole without being split again.
    ///
    /// An item is saved under the unit, the place of this call among the unit's `each` calls, and
    /// [`Item::key`], and under nothing else. Whatever `judge` takes from outside the item and the
    /// view, a table it was built with, a setting, a value worked out before the call, has to be in
    /// the item's key too, or an item judged under one such value is replayed under another.
    ///
    /// The items are handed out in their order to the calling thread, to the run's threads that
    /// are free, and to every thread of the run that finishes its own work while some are not yet
    /// started, so a unit splitting itself while other units are judged gets the threads they
    /// leave and never puts more threads to work than the run has.
    pub fn each<T, F>(&self, items: &[T], judge: F) -> Vec<Vec<LintError>>
    where
        T: Item + Sync,
        F: Fn(&View<'r>, &T) -> Vec<LintError> + Sync,
    {
        let reading = self.reading();
        let call = self.calls.fetch_add(1, Ordering::AcqRel).to_string();
        self.stray.store(false, Ordering::Release);
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
        let stray = self.stray.load(Ordering::Acquire);
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
