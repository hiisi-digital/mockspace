//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether `mock test` runs the benches tree.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Why the benches tree runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Run {
    Asked,
    NeverGreen,
    Moved,
}

/// Why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Skip {
    OtherCratesNamed,
    Unchanged,
}

impl Skip {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Skip::OtherCratesNamed => "crates were named with -p; --benches runs it too",
            Skip::Unchanged => "its inputs have not moved since it last passed; --benches runs it",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Run(Run),
    Skip(Skip),
}

/// Directory names whose contents are build output or version control, never
/// an input of a bench.
const OUTPUT_DIRS: [&str; 2] = ["target", ".git"];

/// A digest of everything under `bench_dir` a bench is built from: each file's
/// path relative to it, then its bytes, in path order. Build output
/// ([`OUTPUT_DIRS`]) is left out, since the bench build writes it and counting
/// it would move the digest on every run.
pub(crate) fn digest(bench_dir: &Path) -> String {
    let mut files = Vec::new();
    collect(bench_dir, &mut files);
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        let rel = f.strip_prefix(bench_dir).unwrap_or(f);
        h.update(rel.to_string_lossy().as_bytes());
        h.update([0u8]);
        h.update(std::fs::read(f).unwrap_or_default());
        h.update([0u8]);
    }
    format!("{:x}", h.finalize())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            let skip = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| OUTPUT_DIRS.contains(&n));
            if !skip {
                collect(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// The benches tree is a tree like the others: asked for, or run when its
/// inputs moved, and never when the caller named crates with `-p` and did not
/// ask. No record means it has never been shown green, so it runs.
pub(crate) fn decide(
    asked: bool,
    crates_named: bool,
    now: &str,
    recorded: Option<&str>,
) -> Verdict {
    if asked {
        return Verdict::Run(Run::Asked);
    }
    if crates_named {
        return Verdict::Skip(Skip::OtherCratesNamed);
    }
    match recorded {
        None => Verdict::Run(Run::NeverGreen),
        Some(r) if r == now => Verdict::Skip(Skip::Unchanged),
        Some(_) => Verdict::Run(Run::Moved),
    }
}

/// The digest the benches tree last passed at, kept under `target/` beside the
/// build it describes: a fresh clone has no build and so no record.
pub(crate) fn recorded(mock_dir: &Path) -> Option<String> {
    std::fs::read_to_string(record_path(mock_dir))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub(crate) fn record_green(mock_dir: &Path, digest: &str) {
    let _ = std::fs::write(record_path(mock_dir), format!("{digest}\n"));
}

fn record_path(mock_dir: &Path) -> PathBuf {
    crate::build_dir::ensure_under_target(mock_dir, &["mockspace-test"]).join("benches-green")
}

#[cfg(test)]
#[path = "test_benches_tests.rs"]
mod tests;
