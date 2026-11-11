//! The ids an instance carries, typed, so that what is not an object id never reaches git.
//!
//! A blob's mode and an instance's base were once tags in the text of an id (`+x` after a blob id,
//! `-` for a base of no commit), and a tag that a caller forgot to take off went to git as part of
//! a name. Each is a type here, and the only way to an [`Oid`] is [`Oid::parse`], which takes
//! nothing but the hex git prints.

use std::fmt;

use crate::error::StoreError;

/// A git object id: 40 or 64 lowercase hex digits, as git prints one. Nothing else is one, so an
/// option, a path or a marker cannot be passed where an object is meant.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Oid(String);

impl Oid {
    /// `text` as an object id, refused where it is anything but the hex of one.
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        let hex = text.bytes().all(|b| b.is_ascii_digit() || (b'a' ..= b'f').contains(&b));
        if matches!(text.len(), 40 | 64) && hex {
            Ok(Self(text.to_owned()))
        } else {
            Err(StoreError::InvalidRef {
                name: text.to_owned(),
                why:  "not an object id",
            })
        }
    }

    /// The id as git prints it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Oid {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<String> for Oid {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

/// The mode a tree gives a file: plain or executable. A symbolic link or a submodule is no file
/// an instance holds, and is refused where a tree is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Plain,
    Exec,
}

impl Mode {
    /// The mode as a tree writes it.
    pub(crate) const fn git(self) -> &'static str {
        match self {
            Self::Plain => "100644",
            Self::Exec => "100755",
        }
    }

    /// The mode of a tree entry, where it is one a file of an instance can have.
    pub(crate) fn from_git(text: &str) -> Option<Self> {
        match text {
            "100644" => Some(Self::Plain),
            "100755" => Some(Self::Exec),
            _ => None,
        }
    }

    pub(crate) const fn of(exec: bool) -> Self {
        if exec { Self::Exec } else { Self::Plain }
    }

    pub(crate) const fn is_exec(self) -> bool {
        matches!(self, Self::Exec)
    }
}

/// What a tree holds at one path: a blob and its mode. Two compare equal where both the content
/// and the mode do, which is what a change of a file is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct FileId {
    pub(crate) blob: Oid,
    pub(crate) mode: Mode,
}

/// What the files of an instance were last brought to: a commit of the ref, or nothing, for an
/// instance of a ref not born yet, which holds no file until its first commit makes the ref.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Base {
    Unborn,
    At(Oid),
}

impl Base {
    /// The commit, where there is one.
    pub fn commit(&self) -> Option<&Oid> {
        match self {
            Self::Unborn => None,
            Self::At(oid) => Some(oid),
        }
    }

    /// Whether this is the base of an instance of a ref not born yet.
    pub fn is_unborn(&self) -> bool {
        matches!(self, Self::Unborn)
    }

    /// The text of the instance's record. Read back only by [`Base::from_record`], and never
    /// given to git.
    pub(crate) fn record_text(&self) -> &str {
        match self {
            Self::Unborn => UNBORN_RECORD,
            Self::At(oid) => oid.as_str(),
        }
    }

    /// The base a record's text names, where it names one.
    pub(crate) fn from_record(text: &str) -> Option<Self> {
        if text == UNBORN_RECORD { Some(Self::Unborn) } else { Oid::parse(text).ok().map(Self::At) }
    }
}

/// How a record writes the base of an instance of a ref not born yet. Not hex, so no record of a
/// commit reads as it.
const UNBORN_RECORD: &str = "-";

impl fmt::Display for Base {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unborn => f.write_str("none, the ref is not born yet"),
            Self::At(oid) => oid.fmt(f),
        }
    }
}

impl PartialEq<String> for Base {
    fn eq(&self, other: &String) -> bool {
        self.commit().is_some_and(|c| c == other)
    }
}
