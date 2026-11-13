//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A map that produces each value at most once and lends it for as long as the map lives.
//!
//! The one primitive every part of the reading is built on. A key asked for the first time gets a
//! slot, the slot is filled by whichever asker reaches it first, and every other asker, on this
//! thread or another, waits for that one and gets the same value. A value is never replaced and
//! never removed, so a reference to it stays good until the map is dropped.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Values produced once per key, lent out for the life of the map.
pub(crate) struct Memo<K, V> {
    slots: Mutex<HashMap<K, Box<OnceLock<V>>>>,
}

impl<K: Eq + Hash, V> Memo<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// The value under `key`, made by `make` if nobody has made it yet.
    ///
    /// The lock is held only to find or add the slot. Making the value happens outside it, so
    /// two keys are made at the same time, and two askers of one key meet at the slot's own
    /// once-cell, where the second waits for the first.
    pub(crate) fn get_or(&self, key: K, make: impl FnOnce() -> V) -> &V {
        self.slot(key).get_or_init(make)
    }

    /// The cell `key`'s value is made in, added empty where the key is new. The cell is boxed and
    /// never removed, so it stays where it is however the map grows and wherever the map itself
    /// is moved, and a reference to it is good for as long as the map lives.
    pub(crate) fn slot(&self, key: K) -> &OnceLock<V> {
        let slot: *const OnceLock<V> = {
            let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            let boxed = slots.entry(key).or_insert_with(|| Box::new(OnceLock::new()));
            &**boxed
        };
        // SAFETY: the slot is boxed, so it does not move when the map grows or the map is moved,
        // and no slot is ever removed or replaced while the map lives. The pointer is therefore
        // valid for as long as `self` is borrowed, which is the lifetime the returned reference
        // carries.
        unsafe { &*slot }
    }

    /// The value under `key` when somebody has made it, without making it.
    pub(crate) fn peek(&self, key: &K) -> Option<&V> {
        let slot: *const OnceLock<V> = {
            let slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
            &**slots.get(key)?
        };
        // SAFETY: as in `get_or`.
        let slot = unsafe { &*slot };
        slot.get()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_key_asked_from_many_threads_at_once_is_made_once() {
        let memo: Memo<u32, String> = Memo::new();
        let made = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0 .. 8 {
                s.spawn(|| {
                    let v = memo.get_or(7, || {
                        made.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        "seven".to_string()
                    });
                    assert_eq!(v, "seven");
                });
            }
        });
        assert_eq!(made.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_lent_value_survives_the_map_growing() {
        let memo: Memo<u32, String> = Memo::new();
        let first = memo.get_or(0, || "zero".to_string());
        for k in 1 .. 1000 {
            memo.get_or(k, || k.to_string());
        }
        assert_eq!(first, "zero");
        assert_eq!(memo.peek(&999).map(String::as_str), Some("999"));
        assert!(memo.peek(&5000).is_none());
    }
}
