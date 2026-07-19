//! ISO-8601 timestamp helper for the IO layer.
//!
//! The canonical implementation lives at [`crate::iso8601::Iso8601Utc`].
//! This module provides a thin String-returning shim for IO call
//! sites that still serialise the timestamp directly into `String`
//! fields (TaskClosure.closed_at, Anchor.captured_at). Once #595's
//! IO carrier retyping lands, these call sites take typed
//! `Iso8601Utc` values and the shim collapses entirely.

use crate::iso8601::Iso8601Utc;

/// Synthesise the current ISO-8601 UTC timestamp as a String for
/// wire-format positions. Delegates to [`Iso8601Utc::now`].
pub(crate) fn current_iso8601() -> String {
    Iso8601Utc::now().as_str().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_iso8601_matches_typed_form() {
        let typed = Iso8601Utc::now();
        let untyped = current_iso8601();
        // Both pull from SystemTime::now() so they may differ by one
        // second on a clock-tick boundary. Compare only the prefix
        // (year-month-day) to avoid the race.
        assert_eq!(&typed.as_str()[.. 10], &untyped[.. 10]);
    }
}
