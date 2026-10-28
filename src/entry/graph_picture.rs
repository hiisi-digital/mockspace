//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether the image of the dependency graph has to be drawn again.
//!
//! The PNG and the SVG are a function of the DOT text, which is a function of the crate graph and
//! the generation header. Drawing the PNG at 150 dpi costs seconds on a graph of a hundred crates,
//! and a commit or a landing that changes no crate has the same DOT text as the one before it, so
//! the image already on disk is the one a redraw would write, provided it is the image that was
//! drawn from that text. Two things can break that without the DOT text moving: a run that wrote a
//! new DOT file and then failed to draw it, and a git operation that replaces the tracked picture
//! (a checkout, a merge keeping one side, a restore) while the untracked stamp stays. So what is
//! kept is a stamp of both, the DOT text and a hash of the picture bytes as written, and the
//! picture is skipped only when both still match the files on disk.

/// The stamp recorded after a successful render: a hash of the picture bytes, a newline, then the
/// DOT text the picture was drawn from.
///
/// The hash is FNV-1a over the bytes, written out in full, so the same bytes give the same stamp in
/// every build of the engine. A stamp that no longer matches only draws the picture again.
#[must_use]
pub(super) fn stamp_of(dot: &str, picture: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in picture {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}\n{dot}")
}

/// True when the picture need not be drawn: it exists (`picture` is its bytes on disk) and the stamp
/// on disk (`stamp`) is the one `stamp_of` gives for the DOT text now on disk and those bytes.
///
/// A missing stamp, a stamp for other DOT text, a stamp for other picture bytes and a missing
/// picture each draw it again. A graphviz upgrade alone does not, since the committed picture is
/// the artifact and a new renderer's different bytes for the same graph are churn.
#[must_use]
pub(super) fn picture_is_current(stamp: Option<&str>, dot: &str, picture: Option<&[u8]>) -> bool {
    match (stamp, picture) {
        (Some(stamp), Some(picture)) => stamp == stamp_of(dot, picture),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{picture_is_current, stamp_of};

    const DOT: &str = "digraph{a->b}";
    const PICTURE: &[u8] = b"png-of-a-b";

    #[test]
    fn an_unchanged_graph_with_the_picture_it_was_drawn_as_is_not_drawn_again() {
        assert!(picture_is_current(Some(&stamp_of(DOT, PICTURE)), DOT, Some(PICTURE)));
    }

    /// The controls: each way the picture is not current must draw it, or the arm above would
    /// pass for a function that always answered true.
    #[test]
    fn a_changed_graph_is_drawn_again() {
        let stamp = stamp_of(DOT, PICTURE);
        assert!(!picture_is_current(Some(&stamp), "digraph{a->b;b->c}", Some(PICTURE)));
    }

    /// A header that differs is a different DOT text, the one place the text carries something
    /// other than the graph.
    #[test]
    fn a_changed_header_over_the_same_graph_is_drawn_again() {
        let stamp = stamp_of("// v1\ndigraph{a}", PICTURE);
        assert!(!picture_is_current(Some(&stamp), "// v2\ndigraph{a}", Some(PICTURE)));
    }

    /// A merge or a checkout put another branch's picture in place while the stamp, in the
    /// untracked target directory, still names the one this clone drew: the bytes differ, so draw.
    #[test]
    fn a_picture_replaced_behind_the_stamp_is_drawn_again() {
        let stamp = stamp_of(DOT, PICTURE);
        assert!(!picture_is_current(Some(&stamp), DOT, Some(b"png-of-another-graph")));
    }

    #[test]
    fn a_graph_never_stamped_is_drawn() {
        assert!(!picture_is_current(None, DOT, Some(PICTURE)));
    }

    #[test]
    fn a_missing_picture_is_drawn_even_with_a_matching_stamp() {
        assert!(!picture_is_current(Some(&stamp_of(DOT, PICTURE)), DOT, None));
    }

    #[test]
    fn the_stamp_of_the_same_bytes_is_the_same_across_calls() {
        assert_eq!(stamp_of(DOT, PICTURE), stamp_of(DOT, PICTURE));
        assert_ne!(stamp_of(DOT, PICTURE), stamp_of(DOT, b"other"));
    }
}
