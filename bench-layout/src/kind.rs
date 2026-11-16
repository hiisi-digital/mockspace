//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The two kinds of bench, and the file that freezes a snapshot.
//!
//! Every bench declares one kind in its `bench.toml`, `kind = "snapshot"` or
//! `kind = "tracking"`, and the kind is a contract on what the bench may build
//! from and on how its results are read.
//!
//! A snapshot is isolated reference code: everything its arms build from sits
//! inside the bench tree, and the files of it are recorded in a
//! [`SNAPSHOT_LOCK`] beside its `bench.toml` and never change after. Its
//! history holds the code fixed, so a moving timing says the machine, the
//! toolchain or the harness moved.
//!
//! A tracking bench builds against living code, at least one arm reaching a
//! crate outside the bench tree, the repository's own code in the usual case.
//! Its history is read across commits, so a moving timing says the code moved.
//!
//! The harness, the tool and the lint pack read the kind through this one
//! vocabulary.

/// The lock file of a composed bench, beside its `bench.toml`.
pub const SNAPSHOT_LOCK: &str = "snapshot.lock";

/// The suffix of a section's lock in a `bench.toml` declaring `[bench.<name>]`
/// sections: `<name>.snapshot.lock`, since one file declares several benches.
pub const SECTION_LOCK_SUFFIX: &str = ".snapshot.lock";

/// The header line of a [`SNAPSHOT_LOCK`].
pub const LOCK_HEADER: &str = "# snapshot lock v1: sha256 and path of every file the snapshot builds from, relative to the bench tree";

/// The `source` a summary row of a tracking bench records: its code is named by
/// the run's commit, not by a lock.
pub const TRACKING_SOURCE: &str = "-";

/// What a bench is for, and so how it builds and how its history is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Isolated reference code, frozen by its lock.
    Snapshot,
    /// Built against living code, compared across commits.
    Tracking,
}

impl Kind {
    /// Every kind, in the order a message lists them.
    pub const ALL: [Kind; 2] = [Kind::Snapshot, Kind::Tracking];

    /// The spelling a `bench.toml` declares.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Snapshot => "snapshot",
            Kind::Tracking => "tracking",
        }
    }

    /// The kind a `bench.toml` spelling names, or `None` for any other word.
    #[must_use]
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// What the kind means, in the words a refusal quotes.
    #[must_use]
    pub const fn contract(self) -> &'static str {
        match self {
            Kind::Snapshot => {
                "a snapshot is isolated reference code: every arm builds only from crates and files \
                 inside the bench tree, and its lock records those files, which never change after"
            },
            Kind::Tracking => {
                "a tracking bench builds against living code: at least one arm reaches a crate \
                 outside the bench tree, and its history is read across commits"
            },
        }
    }

    /// Whether an arm whose manifest the tool generates takes the support
    /// crates at the bench tree root. A tracking arm takes them all, as it
    /// always has; a snapshot arm takes only its own bench's `support/`, since
    /// a crate shared with other benches is changed for their sake, and an arm
    /// that wants one names it in a manifest of its own, which puts it under
    /// the lock.
    #[must_use]
    pub const fn generated_arm_takes_root_support(self) -> bool {
        matches!(self, Kind::Tracking)
    }

    /// How a timing that left its history is called, by kind: a snapshot's
    /// code is fixed, so the environment drifted; a tracking bench's code is
    /// what moved.
    #[must_use]
    pub const fn moved_label(self) -> &'static str {
        match self {
            Kind::Snapshot => "DRIFT",
            Kind::Tracking => "REGRESSION",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The name of a snapshot's lock: [`SNAPSHOT_LOCK`] for a composed bench, which
/// is the only bench its `bench.toml` declares, and `<section>.snapshot.lock`
/// for a `[bench.<section>]` of a file that declares several.
#[must_use]
pub fn lock_name(section: Option<&str>) -> String {
    match section {
        None => SNAPSHOT_LOCK.to_string(),
        Some(s) => format!("{s}{SECTION_LOCK_SUFFIX}"),
    }
}

/// The `source` id of a snapshot whose lock reads `lock`: the first twelve hex
/// digits of the lock's own digest, computed by the caller (this crate has no
/// hash of its own). Two runs compare only where their sources are equal.
#[must_use]
pub fn source_id(lock_digest_hex: &str) -> String {
    lock_digest_hex.chars().take(12).collect()
}

/// Match a `/`-separated glob against a relative path. A whole component of
/// `**` matches any number of components including zero; within a component,
/// `*` matches any run of characters (`bench-*`, `*-probe`), never crossing a
/// `/`. The grammar of `[benchspace] members` and `exclude`, which the harness
/// and the lint pack both resolve.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn component(pat: &str, text: &str) -> bool {
        match pat.split_once('*') {
            None => pat == text,
            Some((prefix, rest)) => {
                let Some(after) = text.strip_prefix(prefix) else {
                    return false;
                };
                (0 ..= after.len())
                    .filter(|i| after.is_char_boundary(*i))
                    .any(|i| component(rest, &after[i ..]))
            },
        }
    }
    fn rec(pat: &[&str], path: &[&str]) -> bool {
        match pat.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => (0 ..= path.len()).any(|skip| rec(rest, &path[skip ..])),
            Some((comp, rest)) => {
                !path.is_empty() && component(comp, path[0]) && rec(rest, &path[1 ..])
            },
        }
    }
    let pat: Vec<&str> = pattern.split('/').filter(|c| !c.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    rec(&pat, &path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind round-trips through its spelling, and no other word, a
    /// capitalised spelling included, names one.
    #[test]
    fn a_kind_is_named_by_its_spelling_and_by_nothing_else() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.name()), Some(k));
        }
        for word in ["", "Snapshot", "TRACKING", "frozen", "living", "snapshots"] {
            assert_eq!(Kind::parse(word), None, "`{word}` named a kind");
        }
    }

    /// A composed bench's lock has the one name, and a section's carries the
    /// section, so two sections of one file never share a lock.
    #[test]
    fn a_section_lock_carries_its_section() {
        assert_eq!(lock_name(None), "snapshot.lock");
        assert_eq!(lock_name(Some("hash")), "hash.snapshot.lock");
        assert_ne!(lock_name(Some("a")), lock_name(Some("b")));
    }

    /// Only a tracking arm takes the root support crates, and the two kinds
    /// call a moved timing by different names.
    #[test]
    fn the_kinds_differ_in_root_support_and_in_what_a_move_is_called() {
        assert!(Kind::Tracking.generated_arm_takes_root_support());
        assert!(!Kind::Snapshot.generated_arm_takes_root_support());
        assert_eq!(Kind::Snapshot.moved_label(), "DRIFT");
        assert_eq!(Kind::Tracking.moved_label(), "REGRESSION");
    }
}
