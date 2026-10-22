//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The three-way merge over blocks.
//!
//! Every block is decided on its own, by key. A block both sides left alone
//! stays as it was, one only one side changed takes that change, one both sides
//! changed the same way is kept once, and one both sides changed differently is
//! a conflict written around that block alone. Nothing here picks a side: an
//! outcome is either every side's change or a conflict.
//!
//! Order is decided afterwards, from the sequences the two sides already hold.
//! Blocks both sides have keep the order of whichever side moved them (ours if
//! both did), and blocks only one side has sit after the block they followed in
//! that side. Where both sides added blocks after the same block, the two runs
//! are ordered by their first key, so the result does not depend on which side
//! is ours.

use std::collections::{HashMap, HashSet};

use super::scan::{Block, Doc, Key};

pub(crate) struct Outcome {
    pub(crate) text:      String,
    pub(crate) conflicts: usize,
    pub(crate) keys:      Vec<Key>,
}

/// What a key resolves to before it is placed.
enum Decision {
    Keep {
        chunk: String,
        tail:  String,
    },
    Conflict {
        ours:   Option<String>,
        theirs: Option<String>,
        tail:   String,
    },
    Gone,
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

fn split_ws(s: &str) -> (&str, &str) {
    let core = s.trim_end();
    (core, &s[core.len() ..])
}

/// The tail of a block both sides carry: whitespace never conflicts, anything
/// else is merged like a chunk. `None` is a conflict.
fn merge_tail(base: Option<&str>, ours: &str, theirs: &str) -> Option<String> {
    if ours == theirs {
        return Some(ours.to_string());
    }
    if base.is_none_or(blank) && blank(ours) && blank(theirs) {
        return Some(ours.to_string());
    }
    if base == Some(ours) {
        return Some(theirs.to_string());
    }
    if base == Some(theirs) {
        return Some(ours.to_string());
    }
    if base.is_some_and(blank) && blank(ours) {
        return Some(theirs.to_string());
    }
    if base.is_some_and(blank) && blank(theirs) {
        return Some(ours.to_string());
    }
    None
}

fn unit(chunk: &str, tail: &str) -> String {
    format!("{chunk}{}", split_ws(tail).0)
}

/// A block only one side still has, against a base that had it: deleted by the
/// other side, which is only clean if this side did not touch it.
fn deleted_by_other(base: &Block, kept: &Block, kept_is_ours: bool) -> Decision {
    let untouched = kept.chunk == base.chunk
        && (kept.tail == base.tail || (blank(&kept.tail) && blank(&base.tail)));
    if untouched {
        return Decision::Gone;
    }
    let side = Some(unit(&kept.chunk, &kept.tail));
    let tail = split_ws(&kept.tail).1.to_string();
    if kept_is_ours {
        Decision::Conflict {
            ours: side,
            theirs: None,
            tail,
        }
    } else {
        Decision::Conflict {
            ours: None,
            theirs: side,
            tail,
        }
    }
}

fn decide(base: Option<&Block>, ours: Option<&Block>, theirs: Option<&Block>) -> Decision {
    match (base, ours, theirs) {
        (_, None, None) => Decision::Gone,
        (None, Some(x), None) | (None, None, Some(x)) => {
            Decision::Keep {
                chunk: x.chunk.clone(),
                tail:  x.tail.clone(),
            }
        },
        (Some(b), Some(x), None) => deleted_by_other(b, x, true),
        (Some(b), None, Some(x)) => deleted_by_other(b, x, false),
        (b, Some(o), Some(t)) => {
            let chunk = match b {
                _ if o.chunk == t.chunk => Some(o.chunk.clone()),
                Some(b) if o.chunk == b.chunk => Some(t.chunk.clone()),
                Some(b) if t.chunk == b.chunk => Some(o.chunk.clone()),
                _ => None,
            };
            let tail = merge_tail(b.map(|b| b.tail.as_str()), &o.tail, &t.tail);
            match (chunk, tail) {
                (Some(chunk), Some(tail)) => {
                    Decision::Keep {
                        chunk,
                        tail,
                    }
                },
                _ => {
                    Decision::Conflict {
                        ours:   Some(unit(&o.chunk, &o.tail)),
                        theirs: Some(unit(&t.chunk, &t.tail)),
                        tail:   split_ws(&o.tail).1.to_string(),
                    }
                },
            }
        },
    }
}

fn marker_block(ours: Option<&str>, theirs: Option<&str>, size: usize) -> String {
    let mut lines = vec![format!("{} ours", "<".repeat(size))];
    lines.extend(ours.map(str::to_string));
    lines.push("=".repeat(size));
    lines.extend(theirs.map(str::to_string));
    lines.push(format!("{} theirs", ">".repeat(size)));
    lines.join("\n")
}

/// The surviving keys of one side, in that side's order.
fn sequence(doc: &Doc, alive: &HashSet<Key>) -> Vec<Key> {
    doc.blocks
        .iter()
        .map(|b| b.key.clone())
        .filter(|k| alive.contains(k))
        .collect()
}

/// For each key not in `backbone`, the key it follows in `seq`, grouped.
fn runs(seq: &[Key], backbone: &HashSet<Key>) -> HashMap<Option<Key>, Vec<Key>> {
    let mut out: HashMap<Option<Key>, Vec<Key>> = HashMap::new();
    let mut anchor: Option<Key> = None;
    for k in seq {
        if backbone.contains(k) {
            anchor = Some(k.clone());
        } else {
            out.entry(anchor.clone()).or_default().push(k.clone());
        }
    }
    out
}

fn order(base: &Doc, ours: &[Key], theirs: &[Key]) -> Vec<Key> {
    let in_theirs: HashSet<&Key> = theirs.iter().collect();
    let in_ours: HashSet<&Key> = ours.iter().collect();
    let common_o: Vec<Key> = ours
        .iter()
        .filter(|k| in_theirs.contains(k))
        .cloned()
        .collect();
    let common_t: Vec<Key> = theirs
        .iter()
        .filter(|k| in_ours.contains(k))
        .cloned()
        .collect();

    let backbone = if common_o == common_t {
        common_o
    } else {
        let in_base: HashSet<&Key> = base.blocks.iter().map(|b| &b.key).collect();
        let base_order: Vec<Key> = base
            .blocks
            .iter()
            .map(|b| &b.key)
            .filter(|k| in_ours.contains(k) && in_theirs.contains(k))
            .cloned()
            .collect();
        let project =
            |v: &[Key]| -> Vec<Key> { v.iter().filter(|k| in_base.contains(k)).cloned().collect() };
        if project(&common_o) == base_order && project(&common_t) != base_order {
            common_t
        } else {
            common_o
        }
    };

    let set: HashSet<Key> = backbone.iter().cloned().collect();
    let mut runs_o = runs(ours, &set);
    let mut runs_t = runs(theirs, &set);
    let mut out = Vec::new();
    let mut place = |anchor: Option<Key>, out: &mut Vec<Key>| {
        let a = runs_o.remove(&anchor).unwrap_or_default();
        let b = runs_t.remove(&anchor).unwrap_or_default();
        let (first, second) = match (a.first(), b.first()) {
            (Some(x), Some(y)) if y < x => (b, a),
            _ => (a, b),
        };
        out.extend(first);
        out.extend(second);
    };
    place(None, &mut out);
    for k in backbone {
        out.push(k.clone());
        place(Some(k), &mut out);
    }
    out
}

fn eol(docs: &[&Doc]) -> &'static str {
    if docs.iter().any(|d| d.render().contains("\r\n")) { "\r\n" } else { "\n" }
}

pub(crate) fn merge(base: &Doc, ours: &Doc, theirs: &Doc, marker: usize) -> Outcome {
    let by = |d: &'_ Doc| -> HashMap<Key, Block> {
        d.blocks
            .iter()
            .map(|b| (b.key.clone(), b.clone()))
            .collect()
    };
    let (bm, om, tm) = (by(base), by(ours), by(theirs));
    let mut keys: Vec<&Key> = bm.keys().chain(om.keys()).chain(tm.keys()).collect();
    keys.sort();
    keys.dedup();

    let mut decisions: HashMap<Key, Decision> = HashMap::new();
    for &k in &keys {
        decisions.insert(k.clone(), decide(bm.get(k), om.get(k), tm.get(k)));
    }
    let alive: HashSet<Key> = decisions
        .iter()
        .filter(|(_, d)| !matches!(d, Decision::Gone))
        .map(|(k, _)| k.clone())
        .collect();
    let placed = order(base, &sequence(ours, &alive), &sequence(theirs, &alive));

    let mut conflicts = 0;
    let head = if ours.head == theirs.head {
        ours.head.clone()
    } else if ours.head == base.head {
        theirs.head.clone()
    } else if theirs.head == base.head {
        ours.head.clone()
    } else {
        conflicts += 1;
        format!(
            "{}\n",
            marker_block(
                Some(ours.head.trim_end_matches('\n')),
                Some(theirs.head.trim_end_matches('\n')),
                marker
            )
        )
    };

    let nl = eol(&[ours, theirs]);
    let spaced = [ours, theirs].iter().any(|d| {
        d.blocks
            .iter()
            .rev()
            .skip(1)
            .any(|b| blank(&b.tail) && b.tail.matches('\n').count() >= 2)
    });
    let mut text = head;
    let mut keys = Vec::new();
    for (i, k) in placed.iter().enumerate() {
        let last = i + 1 == placed.len();
        let (chunk, mut tail) = match decisions.remove(k) {
            Some(Decision::Keep {
                chunk,
                tail,
            }) => (chunk, tail),
            Some(Decision::Conflict {
                ours,
                theirs,
                tail,
            }) => {
                conflicts += 1;
                (
                    marker_block(ours.as_deref(), theirs.as_deref(), marker),
                    tail,
                )
            },
            _ => continue,
        };
        if blank(&tail) {
            let want = if last {
                1
            } else if spaced {
                2
            } else {
                1
            };
            if tail.matches('\n').count() < want {
                tail = nl.repeat(want);
            }
        } else if last && !tail.ends_with('\n') {
            tail.push_str(nl);
        }
        keys.push(k.clone());
        text.push_str(&chunk);
        text.push_str(&tail);
    }
    Outcome {
        text,
        conflicts,
        keys,
    }
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
