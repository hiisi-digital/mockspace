//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a maker may write, as declared and as resolved against one command
//! line.
//!
//! A declared write is either fixed, `CHANGELOG.md` or `docs/gen/**`, or
//! rooted at a declared argument, `{output}.rs` or `{output}/**`, for a tool
//! whose output location is chosen by whoever runs it. A pattern naming an
//! argument constrains nothing until the argument has a value, so the tool
//! also declares, per argument, a fixed root the value has to fall under
//! ([`ArgRoot`]). The engine resolves every pattern from the actual command
//! line before the run, refuses the run where a value leaves its root, and
//! audits what was written against the resolved patterns exactly as it audits
//! fixed ones.
//!
//! Three choices, each closing a way the declaration could say less than it
//! seems to:
//!
//! - **An argument is named only at the start of a pattern, once.** The value
//!   is the anchor and the rest of the pattern a fixed tail, so the root bounds
//!   where every resolved write begins. A placeholder after a wildcard,
//!   `**/{output}`, would put the argument somewhere nothing bounds.
//! - **The root is a pattern the value must match, under the same rules as a
//!   fixed write.** `assets/icons/*` admits one directory level under
//!   `assets/icons`, `assets/icons/**/*` admits any depth, and a root that is
//!   only wildcards is refused for the same reason a write is.
//! - **The value is a plain repository-relative path.** Absolute, `..`, a
//!   `.git` segment, an empty segment or any glob character is refused, the
//!   last because a value of `*` substituted into the pattern would widen it
//!   to whatever the wildcard reaches.

use super::{ArgSpec, Tool, positional_args};
use crate::glob_match_anchored;

/// The fixed root a declared argument's value must fall under, for a maker
/// whose writes name that argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgRoot {
    /// The argument, by its [`ArgSpec::name`].
    pub arg:   &'static str,
    /// A pattern, repository-relative and anchored, that the argument's value
    /// must match. Held to the same rules as a fixed declared write.
    pub under: &'static str,
}

/// Why a fixed pattern, a declared write or a root, is refused, or `None`
/// when it is usable.
///
/// Four shapes are refused:
///
/// - an empty pattern;
/// - one that leaves the repository, absolute or with a `..` segment;
/// - one with a literal `.git` segment, since the engine's observation is `git
///   status`, which never names a path under `.git/`, so a declared write
///   there is one nothing could hold the tool to, and a maker rewriting hooks
///   or config is not something to grant by declaration;
/// - one whose every segment is only wildcards, `*`, `?` or `**` in any
///   arrangement. Such a pattern names no part of the tree and constrains a
///   write by its depth at most: `**/?*` admits every path as surely as `**`
///   does. Declaring one would satisfy "declares what it writes" while saying
///   nothing, which is the same hole as an open [`super::NotALint`].
///
/// A leading argument placeholder counts as a named segment here, since what
/// it resolves to is bounded by its root, which is checked separately.
pub(super) fn refused_pattern(pattern: &str) -> Option<&'static str> {
    let p = pattern.strip_prefix("./").unwrap_or(pattern);
    if p.is_empty() {
        return Some("is empty");
    }
    let segs: Vec<&str> = p.split('/').collect();
    if p.starts_with('/') || segs.contains(&"..") {
        return Some("leaves the repository root");
    }
    if segs.contains(&".git") {
        return Some("has a literal `.git` segment, which the observation cannot see");
    }
    if segs.iter().all(|s| s.chars().all(|c| c == '*' || c == '?')) {
        return Some("is only wildcards");
    }
    None
}

/// The argument a declared write is rooted at, and the fixed tail after it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Rooting<'p> {
    /// No argument: the pattern is fixed.
    Fixed,
    /// `{arg}tail`.
    At {
        arg:  &'p str,
        tail: &'p str,
    },
}

/// Parse where a declared write is rooted, or say why its braces are refused.
///
/// A brace anywhere makes it a placeholder pattern, held to one shape:
/// `{name}` at the very start, a non-empty name, and no brace after it.
pub(super) fn rooting(pattern: &str) -> Result<Rooting<'_>, &'static str> {
    let p = pattern.strip_prefix("./").unwrap_or(pattern);
    if !p.contains(['{', '}']) {
        return Ok(Rooting::Fixed);
    }
    let Some(inner) = p.strip_prefix('{') else {
        return Err(
            "names an argument other than at its start, and may name an argument only at its start",
        );
    };
    let Some((arg, tail)) = inner.split_once('}') else {
        return Err("has an unclosed `{`");
    };
    if arg.is_empty() {
        return Err("names no argument between its braces");
    }
    if arg.contains('{') || tail.contains(['{', '}']) {
        return Err("names an argument more than once, and may name an argument only at its start");
    }
    Ok(Rooting::At {
        arg,
        tail,
    })
}

/// Every way a maker's write declaration is malformed, before anything runs.
///
/// Empty for a check. Called from [`super::contract_faults`].
pub(super) fn declaration_faults(tool: &dyn Tool) -> Vec<String> {
    let super::Purpose::Make {
        writes,
        roots,
    } = tool.purpose()
    else {
        return Vec::new();
    };
    let name = tool.name();
    let declared = |arg: &str| tool.args().iter().any(|a| a.name == arg);
    let mut out = Vec::new();
    let mut named: Vec<&str> = Vec::new();
    for w in writes {
        if let Some(why) = refused_pattern(w) {
            out.push(format!(
                "tool `{name}` declares it writes `{w}`, which {why}. A declared write \
                 has to name part of the repository, or it constrains nothing."
            ));
            continue;
        }
        match rooting(w) {
            Err(why) => {
                out.push(format!(
                    "tool `{name}` declares it writes `{w}`, which {why}."
                ));
            },
            Ok(Rooting::Fixed) => {},
            Ok(Rooting::At {
                arg,
                ..
            }) => {
                if !declared(arg) {
                    out.push(format!(
                        "tool `{name}` declares it writes `{w}`, which names argument \
                         `{arg}`, which the tool does not declare in `args()`."
                    ));
                } else if !roots.iter().any(|r| r.arg == arg) {
                    out.push(format!(
                        "tool `{name}` declares it writes `{w}`, and declares no root for \
                         argument `{arg}`. Without one the argument could point anywhere, \
                         so the write constrains nothing."
                    ));
                }
                named.push(arg);
            },
        }
    }
    for (i, r) in roots.iter().enumerate() {
        let arg = r.arg;
        if roots[.. i].iter().any(|o| o.arg == arg) {
            out.push(format!(
                "tool `{name}` declares a root for argument `{arg}` more than once."
            ));
        } else if !declared(arg) {
            out.push(format!(
                "tool `{name}` declares a root for argument `{arg}`, which the tool does \
                 not declare in `args()`."
            ));
        } else if !named.contains(&arg) {
            out.push(format!(
                "tool `{name}` declares a root for argument `{arg}`, which no declared \
                 write names. A root bounds a write rooted at its argument, so this one \
                 bounds nothing."
            ));
        }
        if let Some(why) = refused_pattern(r.under) {
            out.push(format!(
                "tool `{name}` declares argument `{arg}` falls under `{}`, which {why}. \
                 A root has to name part of the repository, or it bounds nothing.",
                r.under
            ));
        }
    }
    out
}

/// Why an argument's value is not a plain repository-relative path under
/// `under`, or `None` when it is, with the value normalised.
fn refused_value<'v>(value: &'v str, under: &str) -> Result<&'v str, String> {
    let v = value.strip_prefix("./").unwrap_or(value);
    let v = v.strip_suffix('/').unwrap_or(v);
    if v.is_empty() {
        return Err("is empty".to_string());
    }
    let segs: Vec<&str> = v.split('/').collect();
    if v.starts_with('/') || segs.contains(&"..") {
        return Err("leaves the repository root".to_string());
    }
    if segs.contains(&".git") {
        return Err("has a literal `.git` segment".to_string());
    }
    if segs.contains(&"") || segs.contains(&".") {
        return Err("has an empty segment, so it is not a plain path".to_string());
    }
    if v.contains(['*', '?', '[', ']', '{', '}']) {
        return Err(
            "contains a glob character, which would widen the pattern it is put into".to_string(),
        );
    }
    if !glob_match_anchored(under, v) {
        return Err(format!(
            "is not under `{under}`, the root it is declared to fall under"
        ));
    }
    Ok(v)
}

/// What a maker may write on this command line: every declared write with its
/// argument substituted, or every reason the command line is refused.
///
/// A write rooted at an optional argument that was not given resolves to
/// nothing, so the run may write nothing under it. A check resolves to an
/// empty list, since it declares no writes.
///
/// # Errors
///
/// One message per argument value that is not a plain repository-relative
/// path under its declared root.
pub fn resolve_writes(tool: &dyn Tool, args: &[&str]) -> Result<Vec<String>, Vec<String>> {
    let super::Purpose::Make {
        writes,
        roots,
    } = tool.purpose()
    else {
        return Ok(Vec::new());
    };
    let words = positional_args(tool, args);
    let value_of = |arg: &str| {
        let i = tool.args().iter().position(|a: &ArgSpec| a.name == arg)?;
        words.get(i).copied()
    };

    // Each argument checked once, however many writes name it, so a bad value
    // is one message rather than one per pattern.
    let mut values: Vec<(&str, Option<&str>)> = Vec::new();
    let mut refused = Vec::new();
    for r in roots {
        let value = match value_of(r.arg) {
            None => None,
            Some(v) => {
                match refused_value(v, r.under) {
                    Ok(v) => Some(v),
                    Err(why) => {
                        refused.push(format!(
                            "tool `{}` writes under argument `{}`, and its value `{v}` {why}.",
                            tool.name(),
                            r.arg
                        ));
                        None
                    },
                }
            },
        };
        values.push((r.arg, value));
    }
    if !refused.is_empty() {
        return Err(refused);
    }

    let mut out = Vec::new();
    for w in writes {
        match rooting(w) {
            Ok(Rooting::Fixed) => out.push((*w).to_string()),
            Ok(Rooting::At {
                arg,
                tail,
            }) => {
                // Absent, either because it was optional and not given or
                // because the declaration is malformed, which registration
                // refuses before this is reached: nothing may be written
                // under it.
                if let Some((_, Some(v))) = values.iter().find(|(a, _)| *a == arg) {
                    out.push(format!("{v}{tail}"));
                }
            },
            // Refused at registration; resolving it to nothing admits nothing.
            Err(_) => {},
        }
    }
    Ok(out)
}
