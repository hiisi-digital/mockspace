//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock sketches`: every sketch under `<mock>/research/sketches/` with the
//! outcome its `FINDINGS.md` records.
//!
//! A report, never a gate: the sketches already committed predate the
//! `**Outcome:**` line and are records that are not rewritten, so a lint over
//! them would refuse history. It reads, prints and exits zero. `--open` keeps
//! only the sketches that want attention: no `FINDINGS.md`, no outcome line, or
//! a Rust source that times something, which makes it a bench.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::Config;

/// What a sketch's `FINDINGS.md` says it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Works,
    Fails,
    Inconclusive,
    /// An outcome line whose value is none of the three.
    Unclear(String),
    /// A `FINDINGS.md` with no outcome line.
    NoLine,
    /// No `FINDINGS.md` at all.
    NoFindings,
}

impl Outcome {
    fn label(&self) -> String {
        match self {
            Outcome::Works => "WORKS".into(),
            Outcome::Fails => "FAILS".into(),
            Outcome::Inconclusive => "INCONCLUSIVE".into(),
            Outcome::Unclear(v) => format!("unclear outcome: {v}"),
            Outcome::NoLine => "no **Outcome:** line in FINDINGS.md".into(),
            Outcome::NoFindings => "no FINDINGS.md".into(),
        }
    }

    fn recorded(&self) -> bool {
        matches!(self, Outcome::Works | Outcome::Fails | Outcome::Inconclusive)
    }
}

/// One sketch directory, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sketch {
    pub name:    String,
    pub outcome: Outcome,
    /// The first Rust source under it that times something, relative to it.
    pub timer:   Option<PathBuf>,
}

/// The outcome line of a findings text: a line reading `Outcome:`, bold or
/// not, whose first word after it is the outcome. Where the line carries no
/// verdict, the next non-empty line is read for it.
pub fn outcome_of(findings: &str) -> Outcome {
    let mut lines = findings.lines();
    while let Some(line) = lines.next() {
        let bare = line.trim().trim_start_matches(['#', '*', '-', ' ']);
        let Some(head) = bare.get(.."outcome".len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case("outcome") {
            continue;
        }
        let rest = &bare["outcome".len()..];
        let mut rest = rest.trim_start_matches(['*', ':', ' ']);
        // A heading with no verdict on it: the verdict is the next non-empty line.
        if rest.trim().is_empty() {
            rest = lines.find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        }
        let word: String = rest
            .trim_start_matches(['*', '`', ' '])
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        return match word.to_ascii_uppercase().as_str() {
            "WORKS" => Outcome::Works,
            "FAILS" => Outcome::Fails,
            "INCONCLUSIVE" => Outcome::Inconclusive,
            _ => Outcome::Unclear(rest.trim().chars().take(40).collect()),
        };
    }
    Outcome::NoLine
}

/// Whether a Rust source times something: a sketch that does is a bench.
pub fn times_something(source: &str) -> bool {
    ["Instant::now", "std::time::Instant", "criterion::", ".elapsed()"]
        .iter()
        .any(|needle| source.contains(needle))
}

/// Every sketch under `dir`, by name.
pub fn survey(dir: &Path) -> Vec<Sketch> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Sketch> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let path = e.path();
            let outcome = match fs::read_to_string(path.join("FINDINGS.md")) {
                Ok(text) => outcome_of(&text),
                Err(_) => Outcome::NoFindings,
            };
            let timer = rust_sources(&path).into_iter().find(|f| {
                fs::read_to_string(f).is_ok_and(|s| times_something(&s))
            });
            Sketch {
                name: e.file_name().to_string_lossy().into_owned(),
                outcome,
                timer: timer.map(|t| t.strip_prefix(&path).unwrap_or(&t).to_path_buf()),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The `.rs` files under `dir`, build output left out.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                if e.file_name() != "target" && !e.file_name().to_string_lossy().starts_with('.') {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// The report text. `open` keeps only the sketches wanting attention.
pub fn render(sketches: &[Sketch], open: bool) -> String {
    let mut out = String::new();
    for s in sketches {
        if open && s.outcome.recorded() && s.timer.is_none() {
            continue;
        }
        out.push_str(&format!("{}  {}", s.name, s.outcome.label()));
        if let Some(t) = &s.timer {
            out.push_str(&format!("  times something in {}: a bench", t.display()));
        }
        out.push('\n');
    }
    out
}

/// `mock sketches [--open]`.
pub fn cmd(cfg: &Config, args: &[String]) -> ExitCode {
    let mut open = false;
    for a in args {
        match a.as_str() {
            "--open" => open = true,
            other => {
                eprintln!("usage: mock sketches [--open]  (`{other}` is not a flag it takes)");
                return ExitCode::FAILURE;
            },
        }
    }
    let dir = cfg.mock_dir.join("research").join("sketches");
    if !dir.is_dir() {
        println!("no sketches: {} does not exist", dir.display());
        return ExitCode::SUCCESS;
    }
    let all = survey(&dir);
    let open_count = all
        .iter()
        .filter(|s| !s.outcome.recorded() || s.timer.is_some())
        .count();
    print!("{}", render(&all, open));
    println!("{} sketches, {open_count} wanting attention", all.len());
    ExitCode::SUCCESS
}

#[cfg(test)]
#[path = "sketches_tests.rs"]
mod tests;
