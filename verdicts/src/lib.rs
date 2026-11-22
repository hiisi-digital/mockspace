//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Saved verdicts: the one seam a store of them is plugged in by.
//!
//! A check whose verdict depends on inputs it can name asks the store for what it found the last
//! time those inputs were the same, and saves what it finds when they were not. This crate hashes
//! nothing and writes nothing: whoever runs the checks supplies the digest and the store through
//! [`VerdictCache`], and a check that is handed none runs every time.

/// Where saved verdicts live. Implemented by whoever runs the checks.
pub trait VerdictCache: Sync {
    /// A fixed-width digest of `parts`, stable across runs and machines for the same checker.
    ///
    /// The implementation frames the parts (a length before each) so two splittings of the same
    /// bytes give two digests, and mixes in what identifies the code that produced the verdict.
    /// A rebuilt check therefore finds nothing saved by the old.
    fn digest(&self, parts: &[&[u8]]) -> String;

    /// The encoded findings saved under `key`, if any.
    fn replay(&self, key: &str) -> Option<String>;

    /// Save `encoded` under `key`. A store that cannot write says nothing: a verdict not saved
    /// is a verdict recomputed next run, never a wrong one.
    fn remember(&self, key: &str, encoded: &str);

    /// Told of each lookup, for a cache that counts its hits. Counting nothing is the default.
    fn tally(&self, _hit: bool, _what: &str) {}
}
