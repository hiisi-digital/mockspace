//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two branches' test-history files for one member, merged into one.
//!
//! Every landing that reran a member rewrote its file, so every other open
//! branch that had rerun it conflicted at the next merge, and resolving it by
//! hand meant choosing a side and running the member's tests again. `mock
//! merge-test-history` is git's merge driver for those files (see
//! [`crate::bootstrap`] for the wiring), and settles what git conflicted on.
//!
//! **A green record** names the fingerprint a member's suite last passed at. The
//! merged tree has a fingerprint neither side recorded, so a green is kept only
//! where the merge has nothing to decide: both sides hold the same one, or one
//! side left it as the base had it and the other moved it. Where both moved it,
//! to different values, it is dropped, and the next run records it for the tree
//! that exists. The driver never picks one side's green over the other's. A
//! green kept this way is only ever used where it still equals the member's
//! fingerprint, which is how every green is used, so keeping one cannot make a
//! run skip tests the merged tree changed.
//!
//! **A test binary's pass** (`bins`) is the same kind of fact and settles the
//! same way.
//!
//! **Per-test results** are unioned. A test one side has and the other lacks is
//! kept, unless the base had it and the side that still has it left it as it
//! was, which is a deletion on the other side. A test both sides hold
//! differently takes the later measurement: the one side moved when only one
//! did, else the side whose tip commit is later, else (no clock to ask) the
//! longer timing, and a failure over a pass on a tie, since the slower and the
//! failing reading are the ones a selection errs towards.
//!
//! **Refused**, with a message, rather than merged: a side that is not a
//! test-history record, two records of different versions, and two records of
//! different members or flavours. A file is a member's, and a merge across any
//! of those has no meaning to settle.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, ExitCode};

use super::history::{MemberFile, PREVIOUS, TestRecord, VERSION};

/// Which side's measurement is later, as far as the merge could tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Later {
    Ours,
    Theirs,
    Unknown,
}

/// What a merge settled.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub text:          String,
    /// Whether a green record was dropped for want of a value to settle on.
    pub dropped_green: bool,
    /// How many test binaries lost their recorded pass the same way.
    pub dropped_bins:  usize,
}

/// A value after three sides of it: either settled, or moved to two different
/// places from where the base had it.
enum Settled<T> {
    One(Option<T>),
    Both(Option<T>, Option<T>),
}

fn settle<T: PartialEq + Clone>(
    base: &Option<T>,
    ours: &Option<T>,
    theirs: &Option<T>,
) -> Settled<T> {
    if ours == theirs || theirs == base {
        Settled::One(ours.clone())
    } else if ours == base {
        Settled::One(theirs.clone())
    } else {
        Settled::Both(ours.clone(), theirs.clone())
    }
}

/// `green` of the merge of three records, and whether it was dropped. This is the
/// whole of the green rule, shared with the commit guard, which asks of a merge
/// commit whether a null green is what this would have written.
pub fn merged_green<T: PartialEq + Clone>(
    base: &Option<T>,
    ours: &Option<T>,
    theirs: &Option<T>,
) -> (Option<T>, bool) {
    match settle(base, ours, theirs) {
        Settled::One(v) => (v, false),
        Settled::Both(..) => (None, true),
    }
}

/// Three versions of a member's file, merged.
///
/// `base` is whatever git handed as the common ancestor: empty when the two sides
/// added the file, and ignored when it is not a record of the same version,
/// member and flavour, since its fingerprints were made of other inputs.
pub fn merge_text(base: &str, ours: &str, theirs: &str, later: Later) -> Result<Merged, String> {
    let o = MemberFile::parse(ours)
        .map_err(|e| format!("our side is not a test-history record: {e}"))?;
    let t = MemberFile::parse(theirs)
        .map_err(|e| format!("their side is not a test-history record: {e}"))?;
    for (side, f) in [("our", &o), ("their", &t)] {
        if f.version != VERSION && f.version != PREVIOUS {
            return Err(format!(
                "{side} side is test-history version {}, which this build does not read",
                f.version
            ));
        }
    }
    if o.version != t.version {
        return Err(format!(
            "version {} on our side meets version {} on theirs; run `cargo mock test` on one side \
             so both are version {VERSION}, then merge again",
            o.version, t.version
        ));
    }
    if (&o.member, &o.flavour) != (&t.member, &t.flavour) {
        return Err(format!(
            "our side is member `{}` in flavour `{}` and theirs is `{}` in `{}`; one file is one \
             member in one flavour",
            o.member, o.flavour, t.member, t.flavour
        ));
    }
    let b = MemberFile::parse(base)
        .ok()
        .filter(|b| (b.version, &b.member, &b.flavour) == (o.version, &o.member, &o.flavour))
        .unwrap_or_else(|| {
            MemberFile {
                version: o.version,
                flavour: o.flavour.clone(),
                member: o.member.clone(),
                ..MemberFile::default()
            }
        });

    let mut out = MemberFile {
        version: o.version,
        flavour: o.flavour.clone(),
        member: o.member.clone(),
        ..MemberFile::default()
    };
    let (green, dropped_green) = merged_green(&b.green, &o.green, &t.green);
    out.green = green;

    let mut dropped_bins = 0;
    for bin in keys([&b.bins, &o.bins, &t.bins]) {
        let get = |m: &std::collections::BTreeMap<String, String>| m.get(&bin).cloned();
        let (v, dropped) = merged_green(&get(&b.bins), &get(&o.bins), &get(&t.bins));
        dropped_bins += usize::from(dropped);
        if let Some(v) = v {
            out.bins.insert(bin, v);
        }
    }

    for bin in keys([&b.tests, &o.tests, &t.tests]) {
        let empty = Default::default();
        let side = |f: &MemberFile| f.tests.get(&bin).unwrap_or(&empty).clone();
        let (bt, ot, tt) = (side(&b), side(&o), side(&t));
        for name in keys([&bt, &ot, &tt]) {
            let (bv, ov, tv) = (
                bt.get(&name).cloned(),
                ot.get(&name).cloned(),
                tt.get(&name).cloned(),
            );
            let v = match settle(&bv, &ov, &tv) {
                Settled::One(v) => v,
                Settled::Both(o, t) => Some(later_of(o, t, later)),
            };
            if let Some(v) = v {
                out.tests.entry(bin.clone()).or_default().insert(name, v);
            }
        }
    }
    Ok(Merged {
        text: out.render(),
        dropped_green,
        dropped_bins,
    })
}

/// A test both sides moved: a side that dropped it loses to a side that kept it,
/// else the later one.
fn later_of(ours: Option<TestRecord>, theirs: Option<TestRecord>, later: Later) -> TestRecord {
    match (ours, theirs) {
        (Some(o), None) => o,
        (None, Some(t)) => t,
        (Some(o), Some(t)) => {
            match later {
                Later::Ours => o,
                Later::Theirs => t,
                Later::Unknown => {
                    let key = |r: &TestRecord| (r.secs, !r.passed, r.passed_at.clone());
                    if key(&t) > key(&o) { t } else { o }
                },
            }
        },
        (None, None) => unreachable!("moved to two different places, so one of them is a value"),
    }
}

fn keys<K: Ord + Clone, V>(maps: [&std::collections::BTreeMap<K, V>; 3]) -> BTreeSet<K> {
    maps.into_iter().flat_map(|m| m.keys().cloned()).collect()
}

fn read(path: &str) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8")),
        // git hands an empty temp file for a missing base, but be kind to a missing one
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

fn commit_time(rev: &str) -> Option<i64> {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%ct", rev, "--"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// The commits a `git merge` is bringing in. Git does not write `MERGE_HEAD`
/// until the strategy has returned, which is after the driver ran, but it names
/// each one to the strategy as `GITHEAD_<sha>` in the environment.
fn merged_in() -> Vec<String> {
    std::env::vars()
        .filter_map(|(k, _)| k.strip_prefix("GITHEAD_").map(str::to_string))
        .filter(|sha| sha.len() >= 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

/// Which side is later by the tips git is merging: `HEAD` is ours, and the other
/// is the commit (the latest of them, for an octopus) a merge is bringing in, else
/// whichever the operation in progress has recorded. A rebase or a cherry-pick
/// records its commit only once it has stopped, after the driver ran, so there the
/// clock says nothing and the merge falls to the longer timing.
fn later_by_the_clock() -> Later {
    let ours = commit_time("HEAD");
    let named = merged_in().iter().filter_map(|s| commit_time(s)).max();
    let theirs = named.or_else(|| {
        ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REBASE_HEAD", "REVERT_HEAD"]
            .into_iter()
            .find_map(commit_time)
    });
    match (ours, theirs) {
        (Some(o), Some(t)) if o > t => Later::Ours,
        (Some(o), Some(t)) if t > o => Later::Theirs,
        _ => Later::Unknown,
    }
}

/// `args` are what follows the subcommand: `%O %A %B %P [%L]`. Exit 0 is a merge,
/// 1 a refusal that leaves `%A` as it was (git then reports the path as
/// conflicted), and anything higher means the driver did not get to look.
pub(crate) fn run(args: &[&str]) -> ExitCode {
    let [base, ours, theirs, path, ..] = args else {
        eprintln!("usage: mock merge-test-history <base> <ours> <theirs> <path> [<marker size>]");
        return ExitCode::from(2);
    };
    let texts = (read(base), read(ours), read(theirs));
    let (Ok(b), Ok(o), Ok(t)) = texts else {
        let err = [texts.0.err(), texts.1.err(), texts.2.err()]
            .into_iter()
            .flatten()
            .next();
        eprintln!("mock merge-test-history: {}", err.unwrap_or_default());
        return ExitCode::from(2);
    };
    match merge_text(&b, &o, &t, later_by_the_clock()) {
        Ok(m) => {
            if let Err(e) = std::fs::write(Path::new(ours), &m.text) {
                eprintln!("mock merge-test-history: could not write {ours}: {e}");
                return ExitCode::from(2);
            }
            if m.dropped_green || m.dropped_bins > 0 {
                eprintln!(
                    "mock merge-test-history: {path}: both sides recorded a different pass, so {} \
                     dropped; the next `cargo mock test` records it for the merged tree",
                    match (m.dropped_green, m.dropped_bins) {
                        (true, 0) => "the member's green record was".to_string(),
                        (true, n) =>
                            format!("the member's green record and {n} binary pass(es) were"),
                        (false, n) => format!("{n} binary pass(es) were"),
                    }
                );
            }
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!("mock merge-test-history: {path}: {why}");
            ExitCode::from(1)
        },
    }
}

#[cfg(test)]
#[path = "history_merge_tests.rs"]
mod tests;
