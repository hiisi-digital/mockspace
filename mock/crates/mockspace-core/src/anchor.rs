//! Content-addressed anchors (spec §23).
//!
//! An anchor is the per-file content snapshot captured at APPLY entry. Its
//! purpose is to let `mock phase replan` restore the pre-APPLY surface
//! cleanly. The serialized form is two parts:
//!
//! - `.anchor.<side>.toml`: index of `(path, blob_sha)` entries plus header
//!   metadata.
//! - `.anchor.<side>.blobs/<sha-prefix>/<sha-rest>`: the actual blob bytes,
//!   content-addressed by SHA, in git-object-store layout.
//!
//! The on-disk filename IS the expected SHA: integrity verification is a
//! tautology. Restoration recomputes the SHA of the bytes read and compares
//! to the path-encoded SHA; mismatch fires `D004` (see spec §55).

use core::fmt;

use serde::{Deserialize, Serialize};

/// SHA-1 hex length (git's default object format).
pub const SHA1_HEX_LEN: usize = 40;
/// SHA-256 hex length (git's modern object format).
pub const SHA256_HEX_LEN: usize = 64;

/// A validated hex-encoded blob SHA.
///
/// Stores a lowercase hex string of length [`SHA1_HEX_LEN`] or
/// [`SHA256_HEX_LEN`]. Construct via [`BlobSha::parse`] to validate the
/// charset and length.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct BlobSha(String);

/// Why a SHA string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobShaError {
    /// Length is not 40 (SHA-1) or 64 (SHA-256).
    BadLength { len: usize },
    /// A non-hex character appeared.
    BadHexChar { position: usize, found: char },
    /// An uppercase hex character appeared. Lowercase is canonical (git's form).
    UppercaseHex { position: usize, found: char },
}

impl BlobSha {
    /// Parse a hex SHA. Accepts SHA-1 (40 chars) or SHA-256 (64 chars), all
    /// lowercase, hex only.
    pub fn parse(s: &str) -> Result<Self, BlobShaError> {
        let len = s.len();
        if len != SHA1_HEX_LEN && len != SHA256_HEX_LEN {
            return Err(BlobShaError::BadLength { len });
        }
        for (position, ch) in s.chars().enumerate() {
            match ch {
                '0'..='9' | 'a'..='f' => {}
                'A'..='F' => {
                    return Err(BlobShaError::UppercaseHex {
                        position,
                        found: ch,
                    });
                }
                _ => {
                    return Err(BlobShaError::BadHexChar {
                        position,
                        found: ch,
                    });
                }
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The full hex string (40 or 64 chars).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The two-character prefix used as the first directory level in blob
    /// storage (matches git's object-store layout).
    pub fn prefix(&self) -> &str {
        &self.0[..2]
    }

    /// The remaining hex characters after [`Self::prefix`], used as the
    /// blob's filename.
    pub fn rest(&self) -> &str {
        &self.0[2..]
    }

    /// The relative storage path within an `.anchor.<side>.blobs/` tree,
    /// in the form `<prefix>/<rest>`.
    pub fn storage_path(&self) -> String {
        format!("{}/{}", self.prefix(), self.rest())
    }
}

impl fmt::Display for BlobSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for BlobShaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength { len } => {
                write!(
                    f,
                    "SHA length {len} is not {SHA1_HEX_LEN} (SHA-1) or {SHA256_HEX_LEN} (SHA-256)"
                )
            }
            Self::BadHexChar { position, found } => {
                write!(f, "non-hex character {found:?} at position {position}")
            }
            Self::UppercaseHex { position, found } => {
                write!(
                    f,
                    "uppercase hex character {found:?} at position {position}; use lowercase"
                )
            }
        }
    }
}

impl std::error::Error for BlobShaError {}

/// Deserialization for [`BlobSha`] routes through [`BlobSha::parse`] so
/// invalid SHAs fail at TOML-read time, not at first use.
impl<'de> Deserialize<'de> for BlobSha {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        BlobSha::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// One file's snapshot in an anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Source-side path (relative to repo root, forward-slash separated).
    pub path: String,
    /// Hex SHA of the file's content at capture time.
    pub blob_sha: BlobSha,
}

/// The anchor document persisted as `.anchor.<side>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// Schema version of the anchor format.
    pub mockspace_version: String,
    /// ISO-8601 timestamp when the snapshot was taken.
    pub captured_at: String,
    /// Source-side branch tip SHA at capture time (provenance).
    pub captured_from_source_branch_tip: String,
    /// One entry per claimed file.
    #[serde(default, rename = "file")]
    pub files: Vec<FileEntry>,
}

impl Anchor {
    /// Render the anchor as TOML text.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Parse the anchor from TOML text.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1_EXAMPLE: &str = "a1b2c3d4e5f67890123456789012345678901234";
    const SHA256_EXAMPLE: &str = "a1b2c3d4e5f6789012345678901234567890123456789012345678901234beef";

    #[test]
    fn parses_sha1() {
        let s = BlobSha::parse(SHA1_EXAMPLE).unwrap();
        assert_eq!(s.as_str(), SHA1_EXAMPLE);
        assert_eq!(s.prefix(), "a1");
        assert_eq!(s.rest(), "b2c3d4e5f67890123456789012345678901234");
        assert_eq!(
            s.storage_path(),
            "a1/b2c3d4e5f67890123456789012345678901234"
        );
    }

    #[test]
    fn parses_sha256() {
        let s = BlobSha::parse(SHA256_EXAMPLE).unwrap();
        assert_eq!(s.as_str().len(), 64);
        assert_eq!(s.prefix(), "a1");
        assert_eq!(s.rest().len(), 62);
    }

    #[test]
    fn rejects_bad_length() {
        match BlobSha::parse("a1b2") {
            Err(BlobShaError::BadLength { len }) => assert_eq!(len, 4),
            other => panic!("expected BadLength, got {other:?}"),
        }
    }

    #[test]
    fn rejects_uppercase() {
        let mut s = String::from(SHA1_EXAMPLE);
        s.replace_range(3..4, "B");
        match BlobSha::parse(&s) {
            Err(BlobShaError::UppercaseHex { position, .. }) => assert_eq!(position, 3),
            other => panic!("expected UppercaseHex, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_hex() {
        let mut s = String::from(SHA1_EXAMPLE);
        s.replace_range(5..6, "z");
        match BlobSha::parse(&s) {
            Err(BlobShaError::BadHexChar { position, .. }) => assert_eq!(position, 5),
            other => panic!("expected BadHexChar, got {other:?}"),
        }
    }

    #[test]
    fn anchor_toml_round_trip() {
        let anchor = Anchor {
            mockspace_version: "1.0".to_owned(),
            captured_at: "2026-05-18T11:30:00Z".to_owned(),
            captured_from_source_branch_tip: "deadbeefcafebabe0000000000000000deadbeef".to_owned(),
            files: vec![
                FileEntry {
                    path: "crates/foo/src/lib.rs".to_owned(),
                    blob_sha: BlobSha::parse(SHA1_EXAMPLE).unwrap(),
                },
                FileEntry {
                    path: "docs/DESIGN.md".to_owned(),
                    blob_sha: BlobSha::parse(SHA256_EXAMPLE).unwrap(),
                },
            ],
        };

        let serialized = anchor.to_toml().unwrap();
        assert!(serialized.contains("mockspace_version = \"1.0\""));
        assert!(serialized.contains("crates/foo/src/lib.rs"));

        let parsed = Anchor::from_toml(&serialized).unwrap();
        assert_eq!(parsed, anchor);
    }

    #[test]
    fn anchor_toml_rejects_invalid_sha() {
        let bad = r#"
mockspace_version = "1.0"
captured_at = "2026-05-18T11:30:00Z"
captured_from_source_branch_tip = "abc"

[[file]]
path = "src/lib.rs"
blob_sha = "not-a-sha"
"#;
        assert!(Anchor::from_toml(bad).is_err());
    }

    #[test]
    fn anchor_empty_files_round_trips() {
        let anchor = Anchor {
            mockspace_version: "1.0".to_owned(),
            captured_at: "2026-05-18T11:30:00Z".to_owned(),
            captured_from_source_branch_tip: "deadbeef".to_owned(),
            files: vec![],
        };
        let serialized = anchor.to_toml().unwrap();
        let parsed = Anchor::from_toml(&serialized).unwrap();
        assert_eq!(parsed.files.len(), 0);
        assert_eq!(parsed, anchor);
    }
}
