//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Whether a merge result is made of nothing but its parents' lines.
//!
//! Pure over line lists, so the perimeter is pinned without a repository. The
//! caller ([`crate::merge::Merge`]) reads the blobs.
//!
//! Four tests, all of which must hold:
//!
//! - **Counted at the top.** A line may appear at most as many times as the
//!   parents carry it together. A third copy, or a copy of a line only one
//!   parent had once, was typed.
//! - **Counted at the bottom.** A line appears at least as many times as the
//!   parent that carries it least. Deleting one of two identical lines every
//!   parent carries twice is an edit, where deleting a line only some parents
//!   carry is the choice a conflict asks for.
//! - **In sequence.** The staged lines are an interleaving of subsequences of
//!   the parents: each line comes from a parent, in that parent's order. Op's
//!   words for the merge are "preserving both as new sections", which keeps
//!   each side's order and puts the sides one after the other, so a swap of two
//!   lines of one parent, which no side wrote, is not inherited. Sections of
//!   the two sides in either order are, since neither side's own order moved.
//! - **Anchored.** A line the staged content and every parent carry exactly once
//!   keeps its place among the others such lines, in the order every parent has
//!   them. The interleaving above alone cannot see a swap of two lines the
//!   parents share, because either parent could be said to have supplied either.
//!
//! **Bounds of the sequence test.** It is a greedy longest-common-subsequence
//! partition tried from every starting parent, so it can refuse a rare
//! consolidation that only a different partition explains; refusing is the safe
//! direction and the gate says so. It is tried for up to four parents, and for
//! a middle section (after the shared prefix and suffix are stripped) of at most
//! [`CELL_CAP`] cells per parent; past either bound only the two counted tests
//! apply.

use std::collections::HashMap;

/// The largest dynamic-programming table the sequence test builds, in cells.
const CELL_CAP: usize = 4_000_000;

/// The most parents whose orders are tried; an octopus beyond it gets the
/// counted tests only.
const MAX_ORDERED_PARENTS: usize = 4;

/// Whether `staged` only recombines the lines of `parents`, per the module doc.
pub(crate) fn recombines<'a>(staged: &[&'a [u8]], parents: &[Vec<&'a [u8]>]) -> bool {
    let mut ids: HashMap<&'a [u8], u32> = HashMap::new();
    let mut id_of = |line: &'a [u8]| -> u32 {
        let next = ids.len() as u32;
        *ids.entry(line).or_insert(next)
    };
    let staged: Vec<u32> = staged.iter().map(|l| id_of(*l)).collect();
    let parents: Vec<Vec<u32>> = parents
        .iter()
        .map(|p| p.iter().map(|l| id_of(*l)).collect())
        .collect();
    counted(&staged, &parents)
        && anchors_keep_their_order(&staged, &parents)
        && in_sequence(&staged, &parents)
}

/// The tests that only count.
fn counted(staged: &[u32], parents: &[Vec<u32>]) -> bool {
    let tally = |lines: &[u32]| {
        let mut t: HashMap<u32, usize> = HashMap::new();
        for l in lines {
            *t.entry(*l).or_default() += 1;
        }
        t
    };
    let staged = tally(staged);
    let parents: Vec<HashMap<u32, usize>> = parents.iter().map(|p| tally(p)).collect();
    let carried = |id: u32| parents.iter().map(move |p| p.get(&id).copied().unwrap_or(0));
    staged
        .iter()
        .all(|(id, n)| *n <= carried(*id).sum::<usize>())
        && parents
            .iter()
            .flat_map(|p| p.keys())
            .all(|id| staged.get(id).copied().unwrap_or(0) >= carried(*id).min().unwrap_or(0))
}

/// Whether the lines every parent and the staged content carry exactly once
/// stand in the order every parent has them. Without it a swap of two lines the
/// parents share would pass, since either parent could be said to have supplied
/// either line.
fn anchors_keep_their_order(staged: &[u32], parents: &[Vec<u32>]) -> bool {
    let once = |lines: &[u32]| {
        let mut t: HashMap<u32, usize> = HashMap::new();
        for l in lines {
            *t.entry(*l).or_default() += 1;
        }
        t.retain(|_, n| *n == 1);
        t
    };
    let mut anchors = once(staged);
    for p in parents {
        let here = once(p);
        anchors.retain(|id, _| here.contains_key(id));
    }
    let ordered = |lines: &[u32]| -> Vec<u32> {
        lines.iter().copied().filter(|l| anchors.contains_key(l)).collect()
    };
    let spine = ordered(staged);
    parents.iter().all(|p| ordered(p) == spine)
}

/// Whether the staged lines split into one subsequence per parent.
fn in_sequence(staged: &[u32], parents: &[Vec<u32>]) -> bool {
    if parents.len() > MAX_ORDERED_PARENTS {
        return true;
    }
    let mut order: Vec<usize> = (0..parents.len()).collect();
    let mut verdict = false;
    permute(&mut order, 0, &mut |order| {
        if !verdict {
            verdict = leaves_nothing(staged, parents, order);
        }
    });
    verdict
}

/// Takes each parent's longest common subsequence out of what remains, in
/// `order`, and reports whether nothing is left. `true` where the table would
/// pass [`CELL_CAP`], because the sequence test is then not run.
fn leaves_nothing(staged: &[u32], parents: &[Vec<u32>], order: &[usize]) -> bool {
    let mut remaining: Vec<u32> = staged.to_vec();
    for &p in order {
        match take_common(&remaining, &parents[p]) {
            Some(rest) => remaining = rest,
            None => return true,
        }
    }
    remaining.is_empty()
}

/// `lines` without a longest common subsequence of `lines` and `with`, or
/// `None` past the cell cap.
fn take_common(lines: &[u32], with: &[u32]) -> Option<Vec<u32>> {
    let head = lines.iter().zip(with).take_while(|(a, b)| a == b).count();
    let tail = lines[head..]
        .iter()
        .rev()
        .zip(with[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a = &lines[head..lines.len() - tail];
    let b = &with[head..with.len() - tail];
    if a.is_empty() || b.is_empty() {
        return Some(a.to_vec());
    }
    let width = b.len() + 1;
    if (a.len() + 1).saturating_mul(width) > CELL_CAP {
        return None;
    }
    let mut table = vec![0u32; (a.len() + 1) * width];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i * width + j] = if a[i] == b[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut rest = Vec::new();
    while i < a.len() {
        if j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && table[i * width + j + 1] >= table[(i + 1) * width + j] {
            j += 1;
        } else {
            rest.push(a[i]);
            i += 1;
        }
    }
    Some(rest)
}

/// Every ordering of `items[from..]`, handed to `visit`.
fn permute(items: &mut Vec<usize>, from: usize, visit: &mut impl FnMut(&[usize])) {
    if from + 1 >= items.len() {
        visit(items);
        return;
    }
    for i in from..items.len() {
        items.swap(from, i);
        permute(items, from + 1, visit);
        items.swap(from, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<&[u8]> {
        s.split('\n').map(str::as_bytes).collect()
    }

    fn ok(staged: &str, parents: &[&str]) -> bool {
        let parents: Vec<Vec<&[u8]>> = parents.iter().map(|p| split(p)).collect();
        recombines(&split(staged), &parents)
    }

    #[test]
    fn a_blank_line_each_side_brought_may_both_stand() {
        // two appended sections, each with its own separator
        assert!(ok(
            "x\n\n## A\nbody\n\n## B\nbody\n",
            &["x\n\n## A\nbody\n", "x\n\n## B\nbody\n"],
        ));
    }

    #[test]
    fn an_octopus_may_repeat_a_line_as_often_as_the_parents_carry_it_together() {
        let parents = ["h\nx\n", "h\nx\n", "h\nx\n"];
        assert!(ok("h\nx\nx\nx\n", &parents));
        assert!(!ok("h\nx\nx\nx\nx\n", &parents));
    }

    #[test]
    fn five_parents_get_the_counted_tests_only() {
        let first = "x\na\na\nb\nb\n";
        let swapped = "x\nb\nb\na\na\n";
        // repeats put in the other order, which the sequence test refuses,
        // pass past its bound
        assert!(!ok(swapped, &[first, "y"]));
        assert!(ok(swapped, &[first, "y", "y", "y", "y"]));
        assert!(!ok("x\nb\nb\na\na\nzzz\n", &[first, "y", "y", "y", "y"]));
    }

    #[test]
    fn a_table_past_the_cap_is_left_to_the_counted_tests() {
        let side = |tag: &str| -> String { (0..2100).map(|i| format!("{tag}{i}\n")).collect() };
        let (a, b) = (side("a"), side("b"));
        let reversed: String = a.lines().rev().map(|l| format!("{l}\n")).collect();
        assert!(take_common(&[1; 2100], &[2; 2100]).is_none());
        assert!(ok(&format!("{reversed}{b}"), &[&a, &b]));
    }
}
