//! Canonical mockspace agent rule content, embedded at compile time.
//!
//! These markdown files describe mockspace itself: phases, verbs, sides,
//! anchors, suppressions, commands, identity. The content here is the
//! source of truth; the next wiring slice (#581 slice 2) extracts each
//! file to `<repo>/mock/target/agent/<name>.md` on `cargo mock
//! install` / `refresh`. Consumers never see the embedded constants
//! directly; agents read the extracted copies.
//!
//! Design context: `mock/research/202605221200_mockspace-builtin-install-surface-revised.md`.
//!
//! Updating the content: edit the `.md` files in this directory. The
//! `include_str!` macros pull them into the binary at compile time.
//! Rebuild + re-run `cargo mock refresh` in each consumer repo to
//! propagate the new content.

/// Every builtin file in the canonical order they appear in
/// `INDEX.md`, paired with its embedded content. Single source of
/// truth: [`FILE_NAMES`] and [`content`] both derive from this
/// table so adding a file in one place but not the other is
/// impossible.
///
/// The extraction wiring iterates this slice to walk every file
/// the binary ships.
pub const FILES: &[(&str, &str)] = &[
    ("phases.md", include_str!("phases.md")),
    ("verbs.md", include_str!("verbs.md")),
    ("sides.md", include_str!("sides.md")),
    ("anchors.md", include_str!("anchors.md")),
    ("suppressions.md", include_str!("suppressions.md")),
    ("commands.md", include_str!("commands.md")),
    ("identity.md", include_str!("identity.md")),
    ("INDEX.md", include_str!("INDEX.md")),
];

/// File names only, derived from [`FILES`]. Provided as a const
/// for callers that only need the directory listing.
pub const FILE_NAMES: &[&str] = &[
    FILES[0].0, FILES[1].0, FILES[2].0, FILES[3].0, FILES[4].0, FILES[5].0, FILES[6].0, FILES[7].0,
];

/// Look up the embedded content for a builtin file name. Returns
/// `None` when `name` is not in [`FILES`].
///
/// Linear scan over [`FILES`]; the file count is small and fixed,
/// so this is cheaper than a hashmap.
pub fn content(name: &str) -> Option<&'static str> {
    FILES
        .iter()
        .find_map(|(n, body)| (*n == name).then_some(*body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FILES` is the single source of truth for name + content
    /// pairs. Check that every entry has both: a non-empty name and
    /// a non-empty body. A drift between the embedded files and the
    /// `FILES` table is impossible by construction (the names and
    /// `include_str!` calls live on the same line), but the test
    /// catches accidental empties.
    #[test]
    fn every_file_has_name_and_content() {
        for (name, body) in FILES {
            assert!(!name.is_empty(), "empty file name in FILES");
            assert!(!body.is_empty(), "embedded content for `{name}` is empty");
        }
    }

    /// `content` rejects unknown file names.
    #[test]
    fn content_rejects_unknown_names() {
        assert!(content("nonexistent.md").is_none());
        assert!(content("").is_none());
        assert!(
            content("phases").is_none(),
            "no `.md` suffix should not match"
        );
    }

    /// Every builtin file starts with a top-level heading. The renderer
    /// pipeline (slice 2) will care about this; assert it here so a
    /// future edit that strips the heading is caught at test time.
    #[test]
    fn every_file_starts_with_h1() {
        for (name, body) in FILES {
            assert!(
                body.starts_with("# "),
                "builtin file `{name}` does not start with a level-1 heading"
            );
        }
    }

    /// `FILE_NAMES` mirrors the first column of `FILES`. Drift would
    /// signal that the derivation got out of sync.
    #[test]
    fn file_names_match_files_first_column() {
        assert_eq!(FILE_NAMES.len(), FILES.len());
        for (i, name) in FILE_NAMES.iter().enumerate() {
            assert_eq!(*name, FILES[i].0);
        }
    }
}
