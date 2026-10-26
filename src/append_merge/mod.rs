//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock merge-additions`: the merge driver for files that only ever grow.
//!
//! An index of results, a list of `mod` lines, a table of strings, a log of
//! known gaps: each is a file that changes by someone adding to it and by
//! nobody touching what is already there. Two branches that each add to the
//! same spot touch the same lines and conflict as text, though they agree on
//! everything that matters. This driver keeps both additions.
//!
//! Git calls it as `mock merge-additions %O %A %B %P %L`: base, ours, theirs, the
//! path and the conflict marker length, and reads the result from `%A`. Exit 0
//! is a clean merge, exit 1 a conflict with markers in `%A`.
//!
//! **The rule is strict, and everything else is git's own merge.** The driver
//! acts only when neither side changed or removed a single line of the base:
//! each side is the base with lines inserted, and nothing else. Then the
//! insertions are unioned, each in the gap of the base it was made in, and where
//! both sides inserted into one gap, ours comes first and theirs after it, each
//! in its own order. The moment either side edited or removed a base line, or
//! the two sides' insertions into one gap share a line (which reads as the same
//! thing added twice, and keeping both would duplicate it), the file is merged
//! by `git merge-file`, which writes its conflict markers labelled `ours` and
//! `theirs` (git's own merge would label them with the branch names). It never
//! selects a side.
//!
//! A file with no base (added on both sides), or one that does not end in a
//! newline, is also left to git: where an insertion lands at the end of a file
//! without a final newline, joining the two sides' lines is a guess.
//!
//! **Perimeter.** Lines are compared whole, and a base line a side re-inserted
//! elsewhere reads as that side leaving the base alone and adding the line,
//! because the subsequence test cannot tell the two apart. Equal lines are
//! interchangeable in the base, so an insertion made next to one of several
//! equal lines may land beside another of them. The text is the same; it is
//! named so a reader of a diff is not surprised.
//!
//! **A line both sides added, at two different spots, stays twice.** The share
//! test is made per spot, so a line one side put in one gap and the other side
//! put in another is not seen as the same addition, and the file holds it at
//! both. That is what git's own merge does with it, and the driver does not
//! second-guess two gaps it cannot tell apart from two intended lines; a
//! duplicate this makes is for the compiler or a lint of the file's kind to name.
//!
//! **The driver reads lines, not keys.** Two sides that each add a line giving
//! the same TOML key a different value share no line, so both are kept, and the
//! file then holds the key twice, which is not valid TOML. That is again what
//! git's own merge produces, and it is for the parser of the file's kind to
//! refuse; the driver has no notion of a key to refuse it with.

use std::path::Path;
use std::process::{Command, ExitCode};

/// The marker length git uses unless told otherwise.
const DEFAULT_MARKER: usize = 7;

/// Why the driver left a file to git's own merge.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fallback {
    /// A side edited or removed a line of the base.
    EditedBase,
    /// Both sides inserted into one spot and the two share a line.
    SharedAddition,
    /// No base, or a file that does not end in a newline.
    NotAppendShaped,
}

impl Fallback {
    fn describe(&self) -> &'static str {
        match self {
            Fallback::EditedBase => "a side changed a line that was already there",
            Fallback::SharedAddition => "both sides added one of the same lines at one spot",
            Fallback::NotAppendShaped => "no base, or no final newline",
        }
    }
}

fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|b| *b == b'\n').collect()
}

/// For each of the `base.len() + 1` gaps of the base, the lines `side` inserted
/// into it, or `None` when `side` is not the base with lines inserted.
///
/// Leftmost matching: a base line is matched to the first equal line of `side`
/// after the previous match, and the lines passed over are the insertions.
/// Whether `base` is a subsequence of `side` is decided exactly by this.
fn insertions<'a>(base: &[&[u8]], side: &[&'a [u8]]) -> Option<Vec<Vec<&'a [u8]>>> {
    let mut gaps: Vec<Vec<&'a [u8]>> = vec![Vec::new(); base.len() + 1];
    let mut at = 0;
    for (g, line) in base.iter().enumerate() {
        let rest = &side[at ..];
        let found = rest.iter().position(|l| l == line)?;
        gaps[g].extend_from_slice(&rest[.. found]);
        at += found + 1;
    }
    gaps[base.len()].extend_from_slice(&side[at ..]);
    Some(gaps)
}

fn blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

/// Merge three versions of an append-only file's bytes.
pub(crate) fn merge_bytes(base: &[u8], ours: &[u8], theirs: &[u8]) -> Result<Vec<u8>, Fallback> {
    let ends_whole = |t: &[u8]| t.last() == Some(&b'\n');
    if base.is_empty() || ![base, ours, theirs].into_iter().all(ends_whole) {
        return Err(Fallback::NotAppendShaped);
    }
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let (Some(go), Some(gt)) = (insertions(&b, &o), insertions(&b, &t)) else {
        return Err(Fallback::EditedBase);
    };
    let mut out: Vec<u8> = Vec::with_capacity(ours.len() + theirs.len());
    for g in 0 ..= b.len() {
        let (a, c) = (&go[g], &gt[g]);
        if !a.is_empty() && !c.is_empty() && a != c {
            let shares = a.iter().any(|l| !blank(l) && c.iter().any(|m| m == l));
            if shares {
                return Err(Fallback::SharedAddition);
            }
            a.iter()
                .chain(c.iter())
                .for_each(|l| out.extend_from_slice(l));
        } else {
            // one side added here, or both added the very same lines
            let kept = if a.is_empty() { c } else { a };
            kept.iter().for_each(|l| out.extend_from_slice(l));
        }
        if let Some(line) = b.get(g) {
            out.extend_from_slice(line);
        }
    }
    Ok(out)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        // git hands an empty temp file for a missing base, but be kind to a missing one
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

/// The status this driver exits with for what `git merge-file` exited with: 0
/// clean, 1 a conflict, 2 an error.
///
/// `merge-file` exits with the number of conflicts (at most 127), and with 255
/// on an error of its own. Reporting that error as a conflict would hand git a
/// file nobody merged.
fn driver_status(merge_file: Option<i32>) -> u8 {
    match merge_file {
        Some(0) => 0,
        Some(1 ..= 127) => 1,
        _ => 2,
    }
}

/// What git does with no driver: a line-based merge into `ours`, whose markers
/// and exit status are the result.
fn text_merge(base: &str, ours: &str, theirs: &str, marker: usize) -> ExitCode {
    let status = Command::new("git")
        .arg("merge-file")
        .arg(format!("--marker-size={marker}"))
        .args(["-L", "ours", "-L", "base", "-L", "theirs", ours, base, theirs])
        .status();
    let code = status.as_ref().ok().and_then(|s| s.code());
    let exit = driver_status(code);
    if exit == 2 {
        eprintln!("mock merge-additions: git merge-file failed ({status:?})");
    }
    ExitCode::from(exit)
}

/// `args` are what follows the subcommand: `%O %A %B %P [%L]`, the marker size being git's `%L`.
pub(crate) fn run(args: &[&str]) -> ExitCode {
    let [base, ours, theirs, path, rest @ ..] = args else {
        eprintln!("usage: mock merge-additions <base> <ours> <theirs> <path> [<marker size>]");
        return ExitCode::from(2);
    };
    let marker = rest
        .first()
        .and_then(|m| m.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DEFAULT_MARKER);
    let texts = (read(base), read(ours), read(theirs));
    let (Ok(b), Ok(o), Ok(t)) = texts else {
        let err = [texts.0.err(), texts.1.err(), texts.2.err()]
            .into_iter()
            .flatten()
            .next();
        eprintln!("mock merge-additions: {}", err.unwrap_or_default());
        return text_merge(base, ours, theirs, marker);
    };
    match merge_bytes(&b, &o, &t) {
        Ok(merged) => {
            if let Err(e) = std::fs::write(Path::new(ours), merged) {
                eprintln!("mock merge-additions: could not write {ours}: {e}");
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!(
                "mock merge-additions: {path}: {}, merging as text",
                why.describe()
            );
            text_merge(base, ours, theirs, marker)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(base: &str, ours: &str, theirs: &str) -> Result<String, Fallback> {
        merge_bytes(base.as_bytes(), ours.as_bytes(), theirs.as_bytes())
            .map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn two_sections_appended_at_the_end_both_stay_ours_first() {
        let out = merged(
            "# log\n\n## one\nfirst\n",
            "# log\n\n## one\nfirst\n\n## ours\nmine\n",
            "# log\n\n## one\nfirst\n\n## theirs\nyours\n",
        );
        // blank lines are not shared content, so the two blocks stay apart
        assert_eq!(
            out.unwrap(),
            "# log\n\n## one\nfirst\n\n## ours\nmine\n\n## theirs\nyours\n"
        );
    }

    #[test]
    fn additions_in_different_places_each_stay_where_they_were_made() {
        let out = merged(
            "mod a;\nmod c;\nmod e;\n",
            "mod a;\nmod b;\nmod c;\nmod e;\n",
            "mod a;\nmod c;\nmod d;\nmod e;\n",
        );
        assert_eq!(out.unwrap(), "mod a;\nmod b;\nmod c;\nmod d;\nmod e;\n");
    }

    #[test]
    fn two_additions_at_one_spot_keep_each_sides_own_order() {
        let out = merged(
            "mod a;\nmod z;\n",
            "mod a;\nmod b;\nmod c;\nmod z;\n",
            "mod a;\nmod m;\nmod n;\nmod z;\n",
        );
        assert_eq!(
            out.unwrap(),
            "mod a;\nmod b;\nmod c;\nmod m;\nmod n;\nmod z;\n"
        );
    }

    #[test]
    fn a_side_that_changed_nothing_leaves_the_other_sides_file() {
        let out = merged("a\nb\n", "a\nb\n", "a\nb\nc\n");
        assert_eq!(out.unwrap(), "a\nb\nc\n");
        let out = merged("a\nb\n", "a\nb\nc\n", "a\nb\n");
        assert_eq!(out.unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn the_same_addition_on_both_sides_is_kept_once() {
        let out = merged("a\nb\n", "a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(out.unwrap(), "a\nb\nc\n");
    }

    /// The negative the whole rule rests on: a side that edited a line of the
    /// base is not an append, and the file is git's to merge, markers and all.
    #[test]
    fn a_side_that_edited_a_base_line_is_left_to_git() {
        let out = merged("a\nb\n", "a\nB\nours\n", "a\nb\ntheirs\n");
        assert_eq!(out, Err(Fallback::EditedBase));
    }

    #[test]
    fn a_side_that_removed_a_base_line_is_left_to_git() {
        let out = merged("a\nb\nc\n", "a\nc\nours\n", "a\nb\nc\ntheirs\n");
        assert_eq!(out, Err(Fallback::EditedBase));
    }

    #[test]
    fn one_line_added_by_both_sides_at_one_spot_beside_others_is_left_to_git() {
        let out = merged("a\nz\n", "a\nb\nc\nz\n", "a\nc\nd\nz\n");
        assert_eq!(out, Err(Fallback::SharedAddition));
    }

    #[test]
    fn a_file_with_no_base_or_no_final_newline_is_left_to_git() {
        assert_eq!(merged("", "a\n", "b\n"), Err(Fallback::NotAppendShaped));
        assert_eq!(
            merged("a\n", "a\nb", "a\nc\n"),
            Err(Fallback::NotAppendShaped)
        );
    }

    #[test]
    fn a_merge_file_error_is_an_error_and_a_count_of_conflicts_is_a_conflict() {
        assert_eq!(driver_status(Some(0)), 0);
        assert_eq!(driver_status(Some(1)), 1);
        assert_eq!(driver_status(Some(127)), 1);
        assert_eq!(driver_status(Some(255)), 2, "merge-file's own error");
        assert_eq!(driver_status(None), 2, "killed by a signal, or not run");
    }

    /// Pinned, not endorsed: the share test is per spot, so a line added at two
    /// different spots stays twice, as git's own merge leaves it. See the module
    /// perimeter.
    #[test]
    fn the_same_line_added_at_two_different_spots_stays_at_both() {
        let out = merged(
            "mod a;\nmod c;\n",
            "mod a;\nmod b;\nmod c;\n",
            "mod a;\nmod c;\nmod b;\n",
        );
        assert_eq!(out.unwrap(), "mod a;\nmod b;\nmod c;\nmod b;\n");
    }

    /// Pinned, not endorsed: lines are compared whole, so one key given two
    /// values by the two sides stays twice and the file is no longer valid TOML.
    #[test]
    fn the_same_key_given_two_values_stays_twice_and_is_not_valid_toml() {
        let out = merged("[t]\n", "[t]\nk = 1\n", "[t]\nk = 2\n").unwrap();
        assert_eq!(out, "[t]\nk = 1\nk = 2\n");
        assert!(out.parse::<toml_edit::DocumentMut>().is_err(), "the file would have parsed");
    }

    #[test]
    fn bytes_that_are_not_text_are_merged_by_the_same_rule() {
        let base: &[u8] = b"\xff\n";
        let out = merge_bytes(base, b"\xff\n\xfe\n", b"\xff\n\xfd\n").unwrap();
        assert_eq!(out, b"\xff\n\xfe\n\xfd\n");
    }
}
