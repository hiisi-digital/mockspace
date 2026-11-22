//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The values the engine hands over with a context, which a trace names and the run answers.
//!
//! A crate's dependency list, the set of crates, the registry: none of them is a file the reading
//! holds, and a lint reading one through its context writes down the value's name. The run knows
//! every value by name and gives the bytes its digest is taken over, when the trace is saved and
//! when it is asked again.

/// What the run hands over, by name.
pub(crate) trait Values: Sync {
    /// The bytes the named value's digest is taken over, or `None` where the run holds no value of
    /// that name, which no saved trace then verifies against.
    fn material(&self, key: &str) -> Option<Vec<u8>>;
}
