//! Bookkeeping files mockspace owns at ref tree roots (spec §20, §25).
//!
//! When the renderer materialises a ref tree into `.mock/`, it skips
//! bookkeeping entries: the developer never sees these as files in their
//! edit surface. The canonical state stays in the orphan ref tree;
//! [`crate`]'s storage layer caches it for fast read in
//! `.git/mockspace/index.bin`.

use core::fmt;

use crate::phase::ManifestSide;

/// A known bookkeeping file at the ref tree root.
///
/// `.meta` from v1 is intentionally absent: its bookkeeping role is fully
/// subsumed by native git commit metadata (orphan ref commits carry author,
/// timestamp, parent SHA) plus the `[forge]` and `[closed]` blocks in
/// `round.toml`. See spec §25.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BookkeepingFile {
    /// `.phase` (spec §25): phase marker file.
    Phase,
    /// `.anchor.<side>.toml` (spec §23): per-file SHA index captured at APPLY entry.
    AnchorIndex(ManifestSide),
    /// `.anchor.<side>.blobs/`: content-addressed blob storage directory.
    AnchorBlobs(ManifestSide),
}

impl BookkeepingFile {
    /// The on-disk path component (file or directory name).
    pub fn name(self) -> String {
        match self {
            Self::Phase => ".phase".to_owned(),
            Self::AnchorIndex(side) => format!(".anchor.{}.toml", side.marker()),
            Self::AnchorBlobs(side) => format!(".anchor.{}.blobs", side.marker()),
        }
    }
}

impl fmt::Display for BookkeepingFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

/// How a ref-tree root entry classifies for `.mock/` materialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEntry {
    /// User-authored content the renderer surfaces in `.mock/`.
    Editable,
    /// A known bookkeeping shape the renderer filters out.
    Bookkeeping(BookkeepingFile),
    /// A dot-prefixed root entry mockspace does not recognise. Reserved for
    /// future bookkeeping shapes; the renderer filters these out too and
    /// records the encounter so doctor can surface unknown reservations.
    Reserved,
}

/// Classify a ref-tree root entry by its name.
///
/// Intended for ref-tree ROOT entries only. Sub-tree entries (anything under
/// a directory) are not classified here; the renderer recurses into editable
/// subdirectories and prunes at bookkeeping ones.
///
/// Returns `Editable` for the empty string (which should not occur in a
/// well-formed tree, but defensive against caller bugs).
pub fn classify_root_entry(name: &str) -> RootEntry {
    if !name.starts_with('.') {
        return RootEntry::Editable;
    }
    match name {
        ".phase" => RootEntry::Bookkeeping(BookkeepingFile::Phase),
        ".anchor.doc.toml" => {
            RootEntry::Bookkeeping(BookkeepingFile::AnchorIndex(ManifestSide::Doc))
        },
        ".anchor.src.toml" => {
            RootEntry::Bookkeeping(BookkeepingFile::AnchorIndex(ManifestSide::Src))
        },
        ".anchor.doc.blobs" => {
            RootEntry::Bookkeeping(BookkeepingFile::AnchorBlobs(ManifestSide::Doc))
        },
        ".anchor.src.blobs" => {
            RootEntry::Bookkeeping(BookkeepingFile::AnchorBlobs(ManifestSide::Src))
        },
        _ => RootEntry::Reserved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_spec() {
        assert_eq!(BookkeepingFile::Phase.name(), ".phase");
        assert_eq!(
            BookkeepingFile::AnchorIndex(ManifestSide::Doc).name(),
            ".anchor.doc.toml"
        );
        assert_eq!(
            BookkeepingFile::AnchorIndex(ManifestSide::Src).name(),
            ".anchor.src.toml"
        );
        assert_eq!(
            BookkeepingFile::AnchorBlobs(ManifestSide::Doc).name(),
            ".anchor.doc.blobs"
        );
        assert_eq!(
            BookkeepingFile::AnchorBlobs(ManifestSide::Src).name(),
            ".anchor.src.blobs"
        );
    }

    #[test]
    fn classifies_known_bookkeeping() {
        assert_eq!(
            classify_root_entry(".phase"),
            RootEntry::Bookkeeping(BookkeepingFile::Phase)
        );
        assert_eq!(
            classify_root_entry(".anchor.doc.toml"),
            RootEntry::Bookkeeping(BookkeepingFile::AnchorIndex(ManifestSide::Doc))
        );
        assert_eq!(
            classify_root_entry(".anchor.src.blobs"),
            RootEntry::Bookkeeping(BookkeepingFile::AnchorBlobs(ManifestSide::Src))
        );
    }

    #[test]
    fn meta_filename_classifies_as_reserved() {
        // .meta was removed from spec v2.1; if a stale tree still contains
        // one, classification should treat it as reserved (filtered, but
        // not a known bookkeeping shape).
        assert_eq!(classify_root_entry(".meta"), RootEntry::Reserved);
    }

    #[test]
    fn classifies_editable_when_no_dot_prefix() {
        for name in [
            "01_topic.foo.md",
            "manifest.doc.toml",
            "manifest.doc.locked.toml",
            "manifest.src.toml",
            "round.toml",
            "comments",
            "",
        ] {
            assert_eq!(
                classify_root_entry(name),
                RootEntry::Editable,
                "name {name:?} should classify Editable"
            );
        }
    }

    #[test]
    fn classifies_unknown_dot_as_reserved() {
        for name in [".future-marker", ".anchor.doc.extra", ".phase-backup", ".lock"] {
            assert_eq!(
                classify_root_entry(name),
                RootEntry::Reserved,
                "name {name:?} should classify Reserved"
            );
        }
    }

    #[test]
    fn name_round_trips_through_classify() {
        for marker in [
            BookkeepingFile::Phase,
            BookkeepingFile::AnchorIndex(ManifestSide::Doc),
            BookkeepingFile::AnchorIndex(ManifestSide::Src),
            BookkeepingFile::AnchorBlobs(ManifestSide::Doc),
            BookkeepingFile::AnchorBlobs(ManifestSide::Src),
        ] {
            let name = marker.name();
            assert_eq!(
                classify_root_entry(&name),
                RootEntry::Bookkeeping(marker),
                "round-trip failed for {marker:?}"
            );
        }
    }
}
