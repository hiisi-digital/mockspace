//! Which files of its directory an instance holds.
//!
//! An instance of a whole directory holds every regular file in it. One made with
//! [`Instance::holding`](super::Instance::holding) holds only the files its patterns match, so the
//! directory may be one the branch keeps files in as well: a crate's sources beside its
//! templates, an open round's flat files beside the closed rounds.
//!
//! A pattern is a path of `/`-separated segments, relative to the directory. A segment is
//! literal text in which `*` stands for any run of characters, or it is `**`, which stands for
//! any number of whole segments, none included. Nothing else is special, and a character git's
//! ignore rules would read as special is refused. An exclude line matches a directory where a
//! pattern here matches files only, so the exclude file gets each pattern followed by the line
//! that takes directories back out of it.

use crate::error::StoreError;

/// The patterns of an instance. Holds a path when any one of them matches the whole of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    patterns: Vec<Vec<String>>,
}

impl Held {
    /// The patterns, each checked. None at all is refused: an instance that holds nothing is
    /// not one, and the instance of every file is the one made without patterns.
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Result<Self, StoreError> {
        if patterns.is_empty() {
            return Err(refused("", "no pattern is given"));
        }
        let mut out = Vec::new();
        for p in patterns {
            let p = p.as_ref();
            if p.is_empty() {
                return Err(refused(p, "it is empty"));
            }
            if let Some(c) = p.chars().find(|c| c.is_control() || "\\?[]!# ".contains(*c)) {
                return Err(refused(p, match c {
                    ' ' => "it holds a space",
                    '?' | '[' | ']' => "only `*` and `**` are read as wildcards",
                    _ => "it holds a character the exclude file reads as special",
                }));
            }
            let mut segments = Vec::new();
            for seg in p.split('/') {
                match seg {
                    "" => return Err(refused(p, "it has an empty segment, or starts or ends with `/`")),
                    "." | ".." => return Err(refused(p, "it holds `.` or `..`")),
                    s if s.contains("**") && s != "**" => {
                        return Err(refused(p, "`**` stands alone as a segment"));
                    },
                    s => segments.push(s.to_owned()),
                }
            }
            out.push(segments);
        }
        Ok(Self {
            patterns: out,
        })
    }

    /// Whether `path`, `/`-separated and relative to the instance's directory, is held.
    pub fn holds(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        self.patterns.iter().any(|p| matches(p, &parts))
    }

    /// Whether a file under the directory `dir` could be held, so a walk need not enter one
    /// that cannot hold any.
    pub(super) fn may_hold_under(&self, dir: &str) -> bool {
        let parts: Vec<&str> = dir.split('/').collect();
        self.patterns.iter().any(|p| {
            for (i, part) in parts.iter().enumerate() {
                match p.get(i) {
                    None => return false,
                    Some(seg) if seg == "**" => return true,
                    Some(seg) if !segment(seg, part) => return false,
                    Some(_) => {},
                }
            }
            p.len() > parts.len()
        })
    }

    /// The patterns as written, for the exclude file.
    pub fn patterns(&self) -> impl Iterator<Item = String> + '_ {
        self.patterns.iter().map(|p| p.join("/"))
    }
}

fn refused(pattern: &str, why: &'static str) -> StoreError {
    StoreError::Pattern {
        pattern: pattern.to_owned(),
        why,
    }
}

fn matches(pattern: &[String], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((seg, rest)) if seg == "**" => (0 ..= path.len()).any(|skip| {
            // `**` last stands for at least one segment: a pattern names files, and the
            // directory a file is in is not one
            if rest.is_empty() { skip == path.len() && !path.is_empty() } else { matches(rest, &path[skip ..]) }
        }),
        Some((seg, rest)) => {
            match path.split_first() {
                Some((part, more)) => segment(seg, part) && matches(rest, more),
                None => false,
            }
        },
    }
}

/// One segment against one name: `*` is any run of characters, the rest is literal.
fn segment(pattern: &str, name: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else { return false };
    if !pattern.contains('*') {
        return rest.is_empty();
    }
    let pieces: Vec<&str> = pieces.collect();
    for (i, piece) in pieces.iter().enumerate() {
        if i + 1 == pieces.len() {
            return rest.len() >= piece.len() && rest.ends_with(piece);
        }
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len() ..],
            None => return false,
        }
    }
    true
}
