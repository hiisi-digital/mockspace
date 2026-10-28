//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether the image of the dependency graph has to be drawn again.
//!
//! The PNG and the SVG are a function of the DOT text, which is a function of the crate graph and
//! the generation header. Drawing the PNG at 150 dpi costs seconds on a graph of a hundred crates,
//! and a commit or a landing that changes no crate has the same DOT text as the one before it, so
//! the image already on disk is the one a redraw would write.

/// True when `picture` need not be drawn: the DOT text about to be written is byte for byte the
/// one already on disk (`previous_dot`), and the picture drawn from it exists.
///
/// A missing DOT file, a DOT file that differs and a missing picture each draw it again. A
/// graphviz upgrade alone does not, since the committed picture is the artifact and a new
/// renderer's different bytes for the same graph are churn.
#[must_use]
pub(super) fn picture_is_current(
    previous_dot: Option<&str>,
    dot: &str,
    picture_exists: bool,
) -> bool {
    picture_exists && previous_dot == Some(dot)
}

#[cfg(test)]
mod tests {
    use super::picture_is_current;

    #[test]
    fn an_unchanged_graph_with_its_picture_is_not_drawn_again() {
        assert!(picture_is_current(Some("digraph{a->b}"), "digraph{a->b}", true));
    }

    /// The three controls: each of the ways the picture is not current must draw it, or the
    /// arm above would pass for a function that always answered true.
    #[test]
    fn a_changed_graph_is_drawn_again() {
        assert!(!picture_is_current(Some("digraph{a->b}"), "digraph{a->b;b->c}", true));
    }

    #[test]
    fn a_graph_never_written_is_drawn() {
        assert!(!picture_is_current(None, "digraph{a->b}", true));
    }

    #[test]
    fn a_missing_picture_is_drawn_even_for_an_unchanged_graph() {
        assert!(!picture_is_current(Some("digraph{a->b}"), "digraph{a->b}", false));
    }

    /// A header that differs is a different DOT text, so a change of the generation header, the
    /// one place the DOT text carries something other than the graph, draws the picture again.
    #[test]
    fn a_changed_header_over_the_same_graph_is_drawn_again() {
        assert!(!picture_is_current(Some("// v1\ndigraph{a}"), "// v2\ndigraph{a}", true));
    }
}
