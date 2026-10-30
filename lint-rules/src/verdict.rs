//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Saved verdicts: the seam the engine plugs a store into, and what goes into a key.
//!
//! This crate hashes nothing and writes nothing. It says **what a verdict depends on**
//! ([`file_scalars`], [`crate_digest`], [`behaviour_material`]) and **how findings are written
//! down** ([`encode`], [`decode`]); the engine supplies the digest and the store through
//! [`VerdictCache`]. Keeping the hash out of here means a lint pack, which links this crate, gains
//! no dependency from the cache.
//!
//! **A key holds what the lint declared it reads, and no more.** For a lint whose declared reach is
//! honest, that is every input its verdict can depend on, so a hit replays what a run would have
//! found and a miss runs the lint. Honesty of a declaration is sampled by `reach_check`, which moves
//! the inputs a lint claims not to read; it is not proved. The severity a project configures
//! is not an input: it is applied to findings after the lint ran, by the runner, to a replayed
//! finding exactly as to a fresh one, so changing it never invalidates anything.

use crate::{Level, LintConfig, LintContext, LintError, Severity};

/// Where saved verdicts live. Implemented by the engine.
pub trait VerdictCache {
    /// A fixed-width digest of `parts`, stable across runs and machines for the same engine.
    ///
    /// The implementation frames the parts (a length before each) so two splittings of the same
    /// bytes give two digests, and mixes in what identifies the code that produced the verdict:
    /// the engine and every loaded pack. A rebuilt lint therefore finds nothing saved by the old.
    fn digest(&self, parts: &[&[u8]]) -> String;

    /// The encoded findings saved under `key`, if any.
    fn replay(&self, key: &str) -> Option<String>;

    /// Save `encoded` under `key`. A store that cannot write says nothing: a verdict not saved
    /// is a verdict recomputed next run, never a wrong one.
    fn remember(&self, key: &str, encoded: &str);

    /// Note that the lint named `lint` was answered from the store (`hit`) or judged again. The
    /// engine implements it to feed its timing report; it is a method here and not a call into the
    /// timing table because a lint pack carries its own copy of this crate, whose table the engine
    /// never prints.
    fn tally(&self, _hit: bool, _lint: &str) {}
}

/// A store that mixes one lint's behaviour material into every digest it makes, so a verdict a repo
/// lint saves through [`crate::RepoContext::remembered`] is keyed on how that lint was configured
/// without the lint having to say so.
pub(crate) struct Salted<'a> {
    /// The store behind.
    pub inner: &'a dyn VerdictCache,
    /// What goes in front of every digest's parts.
    pub salt:  String,
}

impl VerdictCache for Salted<'_> {
    fn digest(&self, parts: &[&[u8]]) -> String {
        let mut all: Vec<&[u8]> = Vec::with_capacity(parts.len() + 1);
        all.push(self.salt.as_bytes());
        all.extend_from_slice(parts);
        self.inner.digest(&all)
    }

    fn replay(&self, key: &str) -> Option<String> {
        self.inner.replay(key)
    }

    fn remember(&self, key: &str, encoded: &str) {
        self.inner.remember(key, encoded);
    }

    fn tally(&self, hit: bool, lint: &str) {
        self.inner.tally(hit, lint);
    }
}

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

/// What a file-reach lint can see of a context besides the file's text.
///
/// The scalars `Reach::File` is allowed to read: the crate's name and short name, the prefix, the
/// two proc-macro answers, and the primitives this crate introduces. Nothing else of the context.
#[must_use]
pub(crate) fn file_scalars(ctx: &LintContext) -> String {
    let mut introduced: Vec<&str> = ctx
        .primitive_introductions
        .get(ctx.crate_name)
        .map(|l| l.iter().map(String::as_str).collect())
        .unwrap_or_default();
    introduced.sort_unstable();
    format!(
        "crate\0{}\0{}\0{}\0{}\0{}\0{}",
        ctx.crate_name,
        ctx.short_name,
        ctx.crate_prefix,
        ctx.is_proc_macro_crate(),
        ctx.lint_proc_macro_source,
        introduced.join(","),
    )
}

/// The digest of everything a crate-reach lint can see of a context.
///
/// Every source file by path and text, the design document, the concatenated doc templates, the
/// SHAME template, the dependency list, the names of every crate in the workspace, and the
/// scalars a file-reach lint sees. Computed once per crate per run and shared by every lint.
#[must_use]
pub(crate) fn crate_digest(cache: &dyn VerdictCache, ctx: &LintContext) -> String {
    let scalars = file_scalars(ctx);
    let mut deps: Vec<&str> = ctx.deps.iter().map(String::as_str).collect();
    deps.sort_unstable();
    let deps = deps.join("\0");
    let all_crates = ctx.all_crates.iter().map(String::as_str).collect::<Vec<_>>().join("\0");
    let proc_macro_crates = ctx.proc_macro_crates.join("\0");

    let mut parts: Vec<&[u8]> = vec![
        b"crate-digest",
        scalars.as_bytes(),
        deps.as_bytes(),
        all_crates.as_bytes(),
        proc_macro_crates.as_bytes(),
        ctx.design_doc.unwrap_or("\u{1}absent").as_bytes(),
        ctx.all_doc_content.as_bytes(),
        ctx.shame_doc.unwrap_or("\u{1}absent").as_bytes(),
    ];
    let paths: Vec<String> = ctx
        .all_sources
        .iter()
        .map(|f| f.rel_path.to_string_lossy().into_owned())
        .collect();
    for (file, path) in ctx.all_sources.iter().zip(&paths) {
        parts.push(path.as_bytes());
        parts.push(file.text.as_bytes());
    }
    cache.digest(&parts)
}

// ---------------------------------------------------------------------------
// Findings, written down
// ---------------------------------------------------------------------------

/// The findings saved under `key`, or what `compute` finds, saved for next time.
///
/// The one place a verdict is replayed or remembered, so the counts reported for a lint and the
/// rule that an unreadable entry is a miss are the same for a file, a crate, a workspace and a repo
/// item. `compute` answers `None` when there is nothing to judge, which saves and counts nothing.
pub(crate) fn replay_or(
    cache: &dyn VerdictCache,
    key: &str,
    lint: &str,
    compute: impl FnOnce() -> Option<Vec<LintError>>,
) -> Option<Vec<LintError>> {
    if let Some(found) = cache.replay(key).as_deref().and_then(decode) {
        cache.tally(true, lint);
        return Some(found);
    }
    let found = compute()?;
    cache.tally(false, lint);
    cache.remember(key, &encode(&found));
    Some(found)
}

/// The version line of the encoding. A reader that does not know it finds nothing.
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
