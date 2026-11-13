//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Saved verdicts: the seam the engine plugs a store into, what a lint is keyed on, and how
//! findings are written down.
//!
//! This crate hashes nothing and writes nothing to disk itself. The engine supplies the digest and
//! the store through [`VerdictCache`]; [`crate::reading::Store`] keeps each unit's traces and
//! findings through it. Keeping the hash out of here means a lint pack, which links this crate,
//! gains no dependency from the cache. The severity a project configures is not part of a key: the
//! runner applies it to replayed findings exactly as to fresh ones.

use crate::{Level, LintConfig, LintError, Severity};

/// Where saved verdicts live, implemented by the engine: the one trait every check that replays
/// shares. The engine's `tally` feeds its timing report; it is a method of the trait and not a call
/// into the timing table because a lint pack carries its own copy of this crate, whose table the
/// engine never prints.
pub use mockspace_verdicts::VerdictCache;

/// Where a lint came from, which is part of what it is: two lints of one kind can share a name when a
/// pack declares one a built-in does, and they must not share a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Shipped in this crate.
    BuiltIn,
    /// Handed over by a pack.
    Pack,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "builtin",
            Self::Pack => "pack",
        }
    }
}

/// Everything about how a lint is set up that can change what it finds.
///
/// Its parameters, and the path filter that decides which files it is shown. Not its severity
/// (applied afterwards) and not which crates it binds (decides whether it runs, not what it finds).
#[must_use]
pub(crate) fn behaviour_material(lint_name: &str, origin: Origin, cfg: Option<&LintConfig>) -> String {
    let mut out = format!("lint\0{}\0{lint_name}\0", origin.label());
    let Some(cfg) = cfg else { return out };
    if let Some(params) = cfg.params.get(lint_name) {
        let mut keys: Vec<&String> = params.keys().collect();
        keys.sort();
        for k in keys {
            out.push_str(&format!("param\0{k}\0{}\0", params[k]));
        }
    }
    if let Some(filter) = cfg.filter_for(lint_name) {
        for p in &filter.include {
            out.push_str(&format!("include\0{p}\0"));
        }
        for p in &filter.exclude {
            out.push_str(&format!("exclude\0{p}\0"));
        }
    }
    out
}

const FORMAT: &str = "verdict-v2";

/// Findings as text, one per line, readable by [`decode`].
#[must_use]
pub(crate) fn encode(findings: &[LintError]) -> String {
    let mut out = format!("{FORMAT} {}\n", findings.len());
    for f in findings {
        let sev = [f.severity.on_commit, f.severity.on_build, f.severity.on_push]
            .map(level_letter)
            .iter()
            .collect::<String>();
        let fields = [
            escape(&f.crate_name),
            f.line.to_string(),
            escape(f.lint_name),
            sev,
            optional(f.finding_kind),
            optional(f.path.as_deref()),
            escape(&f.message),
        ];
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

/// The findings [`encode`] wrote, or `None` if the text is not exactly that.
///
/// Never a partial list: the first line says how many findings follow, so a file cut at a line
/// boundary, or edited by hand, is a miss and costs a rerun and never a missing finding.
#[must_use]
pub(crate) fn decode(text: &str) -> Option<Vec<LintError>> {
    let mut lines = text.lines();
    let (format, count) = lines.next()?.split_once(' ')?;
    if format != FORMAT {
        return None;
    }
    let count: usize = count.parse().ok()?;
    let mut out = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let [crate_name, lineno, lint_name, sev, kind, path, message] = f[..] else {
            return None;
        };
        let mut levels = sev.chars().map(letter_level);
        let severity = Severity::new(levels.next()??, levels.next()??, levels.next()??);
        if levels.next().is_some() {
            return None;
        }
        out.push(LintError {
            crate_name: unescape(crate_name)?,
            line: lineno.parse().ok()?,
            lint_name: crate::forbidden_imports::leak_rule_name(&unescape(lint_name)?),
            message: unescape(message)?,
            severity,
            finding_kind: unoptional(kind)?.map(|k| crate::forbidden_imports::leak_rule_name(&k)),
            path: unoptional(path)?,
        });
    }
    (out.len() == count).then_some(out)
}

fn level_letter(l: Level) -> char {
    match l {
        Level::Pass => 'P',
        Level::Info => 'I',
        Level::Warn => 'W',
        Level::Error => 'E',
    }
}

fn letter_level(c: char) -> Option<Level> {
    Some(match c {
        'P' => Level::Pass,
        'I' => Level::Info,
        'W' => Level::Warn,
        'E' => Level::Error,
        _ => return None,
    })
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match it.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

/// `~` for none, `=` and the escaped text for some, so an empty string is still a value.
fn optional(s: Option<&str>) -> String {
    match s {
        None => "~".to_string(),
        Some(s) => format!("={}", escape(s)),
    }
}

fn unoptional(s: &str) -> Option<Option<String>> {
    match s.strip_prefix('=') {
        Some(rest) => Some(Some(unescape(rest)?)),
        None if s == "~" => Some(None),
        None => None,
    }
}

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod tests;
