//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A postfix chain on a resolved value: `law.where(status=settled).count()`, `pathof(x).dir()`.

use super::resolved::Resolved;

/// Split `<expr>.verb(arg).verb()` into the expression and its verbs.
///
/// Returns `None` when there is no chain, which is the common case, so the
/// ordinary path is untouched.
///
/// The scan respects parentheses, so a call in the base expression is not cut
/// at the first dot inside it: `pathof(crates::store).dir()` splits after the
/// call. And a chain only starts where a dot is followed by an identifier and
/// an opening parenthesis, so a field holding `seam.toml` is left alone. That
/// second rule is what keeps this from eating ordinary values.
pub(crate) fn split_methods(expr: &str) -> Option<(String, Vec<String>)> {
    let chars: Vec<char> = expr.chars().collect();
    let mut depth = 0usize;
    let mut split_at = None;
    for (i, c) in chars.iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '.' if depth == 0 && looks_like_call(&chars[i + 1 ..]) => {
                split_at = Some(i);
                break;
            },
            _ => {},
        }
    }
    let at = split_at?;
    let base = expr[.. chars[.. at].iter().map(|c| c.len_utf8()).sum::<usize>()]
        .trim()
        .to_string();
    if base.is_empty() {
        return None;
    }
    let rest: String = chars[at + 1 ..].iter().collect();
    let verbs = split_calls(&rest)?;
    if verbs.is_empty() {
        return None;
    }
    Some((base, verbs))
}

/// Whether what follows a dot is `identifier(`.
fn looks_like_call(rest: &[char]) -> bool {
    let mut i = 0;
    while i < rest.len() && (rest[i].is_ascii_alphanumeric() || rest[i] == '_') {
        i += 1;
    }
    i > 0 && rest.get(i) == Some(&'(')
}

/// Split `verb(arg).verb()` into its calls, respecting nesting in arguments.
fn split_calls(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            },
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
                if depth == 0 {
                    out.push(cur.trim().to_string());
                    cur.clear();
                }
            },
            '.' if depth == 0 => {},
            _ => cur.push(c),
        }
    }
    // Trailing text outside any call means the chain was malformed, and a
    // malformed chain is reported rather than half-applied.
    if !cur.trim().is_empty() {
        return None;
    }
    Some(out)
}

/// Narrow what an expression resolved to.
///
/// The methods are the query language, and every one of them is valid in a
/// template too: a filtered table renders as a markdown table exactly as the
/// unfiltered one does. That is the point of resolving to a value rather than
/// to a string. A query worked out at a prompt pastes into a document.
///
/// An unknown method is an error rather than a pass-through: a silently ignored
/// method reads as working, and the contract is that a reference which does not
/// resolve is reported.
pub(crate) fn apply_methods(value: Resolved, methods: &[String]) -> Option<Resolved> {
    let mut v = value;
    for m in methods {
        let (name, arg) = match m.split_once('(') {
            Some((n, a)) => (n.trim(), a.trim_end_matches(')').trim()),
            None => (m.trim(), ""),
        };
        v = match (name, &v) {
            // Table verbs.
            (
                "where",
                Resolved::Table {
                    namespace,
                    columns,
                    rows,
                },
            ) => {
                let (field, want, exact) = parse_predicate(arg)?;
                let col = columns.iter().position(|c| *c == field)?;
                let kept: Vec<Vec<String>> = rows
                    .iter()
                    .filter(|r| {
                        let cell = r.get(col).map(String::as_str).unwrap_or("");
                        if exact { cell == want } else { cell.contains(&want) }
                    })
                    .cloned()
                    .collect();
                Resolved::Table {
                    namespace: namespace.clone(),
                    columns:   columns.clone(),
                    rows:      kept,
                }
            },
            (
                "select",
                Resolved::Table {
                    namespace,
                    columns,
                    rows,
                },
            ) => {
                let wanted: Vec<&str> = arg
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                let idx: Vec<usize> = wanted
                    .iter()
                    .map(|w| columns.iter().position(|c| c == w))
                    .collect::<Option<Vec<_>>>()?;
                Resolved::Table {
                    namespace: namespace.clone(),
                    columns:   wanted.iter().map(|w| w.to_string()).collect(),
                    rows:      rows
                        .iter()
                        .map(|r| idx.iter().map(|i| r[*i].clone()).collect())
                        .collect(),
                }
            },
            (
                "count",
                Resolved::Table {
                    rows,
                    ..
                },
            ) => Resolved::Count(rows.len()),
            // A single row of a table stays a table, so it keeps its column
            // headings and composes with the same verbs the whole table takes.
            (
                "first",
                Resolved::Table {
                    namespace,
                    columns,
                    rows,
                },
            ) => {
                Resolved::Table {
                    namespace: namespace.clone(),
                    columns:   columns.clone(),
                    rows:      rows.first().cloned().into_iter().collect(),
                }
            },
            (
                "last",
                Resolved::Table {
                    namespace,
                    columns,
                    rows,
                },
            ) => {
                Resolved::Table {
                    namespace: namespace.clone(),
                    columns:   columns.clone(),
                    rows:      rows.last().cloned().into_iter().collect(),
                }
            },

            // Path verbs, on whatever the value reads as.
            ("dir", _) => {
                Resolved::Path(
                    v.as_scalar()
                        .rsplit_once('/')
                        .map(|(d, _)| d.to_string())
                        .unwrap_or_default(),
                )
            },
            ("filename", _) => {
                let s = v.as_scalar();
                Resolved::Path(s.rsplit_once('/').map(|(_, f)| f.to_string()).unwrap_or(s))
            },
            ("stem", _) => {
                let s = v.as_scalar();
                let f = s.rsplit_once('/').map(|(_, f)| f.to_string()).unwrap_or(s);
                Resolved::Path(
                    f.rsplit_once('.')
                        .map(|(st, _)| st.to_string())
                        .unwrap_or(f),
                )
            },
            ("ext", _) => {
                Resolved::Path(
                    v.as_scalar()
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_string())
                        .unwrap_or_default(),
                )
            },

            // List verbs.
            ("first", _) => {
                Resolved::Field(crate::refpath::list(&v.as_scalar()).next().unwrap_or("").to_string())
            },
            ("last", _) => {
                Resolved::Field(crate::refpath::list(&v.as_scalar()).last().unwrap_or("").to_string())
            },
            ("count", _) => {
                Resolved::Count(
                    crate::refpath::list(&v.as_scalar()).count(),
                )
            },
            _ => return None,
        };
    }
    Some(v)
}

/// `field=value` for an exact match, `field~value` for a substring.
///
/// Two operators rather than one because both questions are common: which rows
/// name exactly this crate, and which rows mention it at all.
fn parse_predicate(arg: &str) -> Option<(String, String, bool)> {
    if let Some((f, v)) = arg.split_once('~') {
        return Some((f.trim().to_string(), v.trim().to_string(), false));
    }
    let (f, v) = arg.split_once('=')?;
    Some((f.trim().to_string(), v.trim().to_string(), true))
}
