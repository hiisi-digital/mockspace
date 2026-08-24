//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the launcher reads out of a repo's `mockspace.toml`, declared as a
//! type so the engine can see it.
//!
//! The pin lives in the config the engine also reads, and the launcher reads it
//! before the engine exists. That leaves the engine's config gate looking at
//! top-level keys nothing in its own schema declares, which it would otherwise
//! report as unknown and fail every project on. [`ManifestHeader`] is the
//! declaration that closes that: the engine reflects over it for the key names
//! and adds them to what a config may carry.
//!
//! Parsing those keys and acting on them is the launcher's, and the launcher's
//! machinery is `renki`. Nothing here resolves anything.

pub mod gate;

use serde::Deserialize;

/// The top-level `mockspace.toml` keys the launcher reads and the engine does
/// not.
///
/// serde reads only section-less keys into these fields, matching the pin
/// contract's top-level-only rule, and ignores everything else a config
/// carries.
// Serialize as well as Deserialize, so the engine derives the key set from the
// type rather than retyping it.
#[derive(Debug, Default, Clone, Deserialize, serde::Serialize)]
pub struct ManifestHeader {
    /// The workspace directory the config maps.
    #[serde(default)]
    pub mock_dir:          Option<String>,
    /// The engine's source, when the repo does not want the canonical one.
    #[serde(default)]
    pub mockspace_git:     Option<String>,
    /// A released engine, which is both a git tag and a crates.io release.
    #[serde(default)]
    pub mockspace_version: Option<String>,
    /// An explicit rev.
    #[serde(default)]
    pub mockspace_rev:     Option<String>,
    /// An explicit branch, re-resolved to a rev on each run.
    #[serde(default)]
    pub mockspace_branch:  Option<String>,
    /// An explicit tag that is not a crates.io release.
    #[serde(default)]
    pub mockspace_tag:     Option<String>,
}

impl ManifestHeader {
    /// Read a `mockspace.toml`'s launcher keys. Unreadable TOML yields the
    /// all-absent header rather than an error, since a caller treats "no keys"
    /// and "unparseable" the same and the file's real validation is the
    /// engine's.
    pub fn parse(text: &str) -> ManifestHeader {
        toml::from_str(text).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_top_level_keys_are_read() {
        let h = ManifestHeader::parse("mock_dir = \"mock\"\n[table]\nmockspace_version = \"9\"\n");
        assert_eq!(h.mock_dir.as_deref(), Some("mock"));
        assert_eq!(
            h.mockspace_version, None,
            "a pin inside a table is not the top-level pin"
        );
    }

    #[test]
    fn a_broken_file_yields_an_absent_header_rather_than_a_half_read_one() {
        // deliberate: a syntax error anywhere makes the whole header absent, so
        // a valid pin followed by a broken table is lost rather than half-read.
        // Locked here so the behaviour is a decision and not an accident.
        let h = ManifestHeader::parse("mockspace_version = \"0.0.0-d05\"\n[table\nbad = ");
        assert!(h.mockspace_version.is_none());
        assert!(h.mock_dir.is_none());
        // the control: the same pin in a well-formed file does arrive.
        assert_eq!(
            ManifestHeader::parse("mockspace_version = \"0.0.0-d05\"\n")
                .mockspace_version
                .as_deref(),
            Some("0.0.0-d05")
        );
    }
}
