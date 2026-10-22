//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock merge-registry`: the merge driver for registry TOML.
//!
//! Two branches that each added a row to the same spot of a registry file
//! conflict as text, because neither had the other's row in mind and the lines
//! they touched are the same lines. They do not conflict as rows. This driver
//! merges by what a row is, its namespace and `id`, so concurrent additions
//! both stay and only a row changed two different ways reaches a person.
//!
//! Git calls it as `mock merge-registry %O %A %B %P %L`: base, ours, theirs, the
//! path and the conflict marker length, and reads the result from `%A`. Exit 0 is a clean
//! merge, exit 1 a conflict whose markers sit in `%A` around the one block concerned. A file the
//! driver cannot cut into rows (invalid TOML, an entry with no `id`, a row
//! twice) is merged by `git merge-file`, which is what git does without a
//! driver, so the driver is never worse than none.
//!
//! The merge keys on the `[[namespace]]` key and the `id`, as the loader does,
//! and ignores which file a row sits in. It cannot see across files: a row
//! added under one id in two different files by two branches is two additions
//! to it and the registry's duplicate check is what reports it.

mod merge;
mod scan;

use std::path::Path;
use std::process::{Command, ExitCode};

use scan::Doc;

/// The marker length git uses unless told otherwise.
const DEFAULT_MARKER: usize = 7;

/// Merge three versions of a registry file's text.
///
/// `Err` is a file this driver does not handle, and the caller falls back to a
/// text merge. The result is only returned if it cuts into the same rows the
/// merge decided on, so a merge that produced something unreadable is also an
/// `Err` rather than a bad file.
pub(crate) fn merge_text(
    base: &str,
    ours: &str,
    theirs: &str,
    marker: usize,
) -> Result<merge::Outcome, scan::Unsupported> {
    let (b, o, t) = (Doc::parse(base)?, Doc::parse(ours)?, Doc::parse(theirs)?);
    let out = merge::merge(&b, &o, &t, marker);
    if out.conflicts == 0 {
        let again = Doc::parse(&out.text).map_err(|e| {
            scan::Unsupported(format!("the merged text does not read back: {}", e.0))
        })?;
        if again.keys().into_iter().cloned().collect::<Vec<_>>() != out.keys {
            return Err(scan::Unsupported(
                "the merged text does not cut into the rows it was built from".into(),
            ));
        }
    }
    Ok(out)
}

fn read(path: &str) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8")),
        // git hands an empty temp file for a missing base, but be kind to a missing one
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

/// What git does with no driver: a line-based merge into `ours`.
fn text_merge(base: &str, ours: &str, theirs: &str, marker: usize) -> ExitCode {
    let status = Command::new("git")
        .arg("merge-file")
        .arg(format!("--marker-size={marker}"))
        .args(["-L", "ours", "-L", "base", "-L", "theirs", ours, base, theirs])
        .status();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(e) => {
            eprintln!("mock merge-registry: could not run git merge-file: {e}");
            ExitCode::from(2)
        },
    }
}

/// `args` are what follows the subcommand: `%O %A %B %P [%L]`, the marker size being git's `%L`.
pub(crate) fn run(args: &[&str]) -> ExitCode {
    let [base, ours, theirs, path, rest @ ..] = args else {
        eprintln!("usage: mock merge-registry <base> <ours> <theirs> <path> [<marker size>]");
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
        eprintln!("mock merge-registry: {}", err.unwrap_or_default());
        return text_merge(base, ours, theirs, marker);
    };
    match merge_text(&b, &o, &t, marker) {
        Ok(out) => {
            if let Err(e) = std::fs::write(Path::new(ours), &out.text) {
                eprintln!("mock merge-registry: could not write {ours}: {e}");
                return ExitCode::from(2);
            }
            if out.conflicts > 0 {
                eprintln!(
                    "mock merge-registry: {path}: {} block(s) changed on both sides",
                    out.conflicts
                );
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!("mock merge-registry: {path}: {}, merging as text", why.0);
            text_merge(base, ours, theirs, marker)
        },
    }
}
