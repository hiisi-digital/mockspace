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
//! - **Counted at the top, by the base.** A line may appear at most
//!   `base + Σ(parent − base)` times, never fewer than the most any one parent
//!   carries: each side's additions to the base count once. Two sides that each
//!   add a blank separator line both stand, and a hand-typed second copy of a
//!   line both parents carry once does not, since neither side added it. With
//!   the path absent at the base (a file added on both sides) the base counts are
//!   zero and the bound is the sum of the parents. A merge base that cannot be
//!   found at all is not that case: the caller refuses the file before it gets
//!   here.
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
//!   keeps its place among the others such lines, in the whole order of one
//!   parent: the one order the parents share, or either side's where one side
//!   moved a shared line and the other kept it, never a mix of the two. The
//!   interleaving above alone cannot see a swap of two lines the
//!   parents share, because either parent could be said to have supplied either.
//!
//! **Bounds of the sequence test, and none of them lets a file through
//! unchecked.** It is a greedy longest-common-subsequence split (linear space,
//! quadratic time) tried from every starting parent, so it can refuse a rare
//! consolidation that only a different split explains; refusing is the safe
//! direction. With up to four parents every ordering of the parents is tried
//! (24 for four); with more only the given order and its reverse are, so a
//! larger octopus is refused where another order would explain it.
//!
//! The time is capped per comparison, not per file: a comparison whose
//! stripped middle is past [`TIME_CAP`] cells for one parent is not skipped but
//! reported [`Verdict::TooLarge`], which the caller refuses with a message
//! saying why. One file can run several comparisons, one per parent for each
//! ordering tried and each pass of the check, so the cap bounds each of them
//! and not the time the file as a whole takes.

use std::collections::HashMap;

/// The most cells (lines of the staged middle times lines of the parent's) one
/// longest-common-subsequence pass will work through. The time is the bound,
/// since the space is linear. It is a cap per comparison: a file that fails the
/// check can run up to two parents times two orders times the passes of each,
/// or 24 orderings of four parents times the passes of each, and past the cap
/// the file is refused rather than passed unchecked.
const TIME_CAP: usize = 100_000_000;

/// The most parents whose every ordering is tried.
const MAX_PERMUTED_PARENTS: usize = 4;

/// What the line tests found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Nothing the parents did not carry, in order.
    Recombines,
    /// A line was typed, repeated, deleted or moved, and which test said so.
    Authored(Why),
    /// The order test would take longer than [`TIME_CAP`] allows, so the file
    /// is not judged inherited.
    TooLarge,
}

/// Which line test found a merge authored, so the refusal can say it.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Why {
    /// A line no parent carries, a repeat past the bound, or a line every
    /// parent carries that is gone.
    Counted,
    /// The lines the parents share stand in an order no single parent has.
    Order,
    /// The lines do not split into one subsequence per parent.
    Sequence,
}

impl Why {
    /// The sentence a refusal prints for it.
    pub(crate) const fn says(self) -> &'static str {
        match self {
            Why::Counted => {
                "a line neither parent carries, a line repeated beyond what the parents carry \
                 together, or a line every parent carries that is gone"
            },
            Why::Order => "the lines the parents share, in an order no single parent has them in",
            Why::Sequence => "lines that do not keep each parent's own order",
        }
    }
}

/// Whether `staged` only recombines the lines of `parents`, per the module doc.
/// `base` is the merge base's lines, empty where there is none.
pub(crate) fn recombines<'a>(
    staged: &[&'a [u8]],
    parents: &[Vec<&'a [u8]>],
    base: &[&'a [u8]],
) -> Verdict {
    let mut ids: HashMap<&'a [u8], u32> = HashMap::new();
    let mut id_of = |line: &'a [u8]| -> u32 {
        let next = ids.len() as u32;
        *ids.entry(line).or_insert(next)
    };
    let staged: Vec<u32> = staged.iter().map(|l| id_of(*l)).collect();
    let base: Vec<u32> = base.iter().map(|l| id_of(*l)).collect();
    let parents: Vec<Vec<u32>> = parents
        .iter()
        .map(|p| p.iter().map(|l| id_of(*l)).collect())
        .collect();
    if !counted(&staged, &parents, &base) {
        return Verdict::Authored(Why::Counted);
    }
    if !anchors_keep_their_order(&staged, &parents) {
        return Verdict::Authored(Why::Order);
    }
    match in_sequence(&staged, &parents) {
        Ok(true) => Verdict::Recombines,
        Ok(false) => Verdict::Authored(Why::Sequence),
        Err(TooLarge) => Verdict::TooLarge,
    }
}

fn tally(lines: &[u32]) -> HashMap<u32, usize> {
    let mut t: HashMap<u32, usize> = HashMap::new();
    for l in lines {
        *t.entry(*l).or_default() += 1;
    }
    t
}

/// The tests that only count.
fn counted(staged: &[u32], parents: &[Vec<u32>], base: &[u32]) -> bool {
    let (staged, base) = (tally(staged), tally(base));
    let parents: Vec<HashMap<u32, usize>> = parents.iter().map(|p| tally(p)).collect();
    let in_parent = |id: u32| parents.iter().map(move |p| p.get(&id).copied().unwrap_or(0));
    let top = |id: u32| -> usize {
        let b = base.get(&id).copied().unwrap_or(0) as i64;
        let added: i64 = in_parent(id).map(|n| n as i64 - b).sum();
        let by_base = (b + added).max(0) as usize;
        by_base.max(in_parent(id).max().unwrap_or(0))
    };
    staged.iter().all(|(id, n)| *n <= top(*id))
        && parents
            .iter()
            .flat_map(|p| p.keys())
            .all(|id| staged.get(id).copied().unwrap_or(0) >= in_parent(*id).min().unwrap_or(0))
}

/// Whether the lines every parent and the staged content carry exactly once
/// stand in the order some parent has them, the whole of that order. Without it
/// a swap of two lines the parents share would pass, since either parent could
/// be said to have supplied either line. Where the parents agree that order is
/// theirs; where one side moved a line the other kept, the conflict asks for a
/// choice between the sides, so either side's order stands. A mix of the two is
/// refused, since stitching orders pair by pair puts a line where no side had
/// it, and so is a conflicted merge where each side moved different shared
/// lines, which is the safe direction.
fn anchors_keep_their_order(staged: &[u32], parents: &[Vec<u32>]) -> bool {
    let once = |lines: &[u32]| {
        let mut t = tally(lines);
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
    parents.iter().any(|p| ordered(p) == spine)
}

/// The sequence test would pass [`TIME_CAP`].
struct TooLarge;

/// Whether the staged lines split into one subsequence per parent.
fn in_sequence(staged: &[u32], parents: &[Vec<u32>]) -> Result<bool, TooLarge> {
    let mut order: Vec<usize> = (0..parents.len()).collect();
    if parents.len() > MAX_PERMUTED_PARENTS {
        let given = leaves_nothing(staged, parents, &order)?;
        order.reverse();
        return Ok(given || leaves_nothing(staged, parents, &order)?);
    }
    let mut verdict: Result<bool, TooLarge> = Ok(false);
    permute(&mut order, 0, &mut |order| {
        if !matches!(verdict, Ok(false)) {
            return;
        }
        verdict = leaves_nothing(staged, parents, order);
    });
    verdict
}

/// Takes each parent's longest common subsequence out of what remains, in
/// `order`, and reports whether nothing is left.
fn leaves_nothing(staged: &[u32], parents: &[Vec<u32>], order: &[usize]) -> Result<bool, TooLarge> {
    let mut remaining: Vec<u32> = staged.to_vec();
    for &p in order {
        remaining = take_common(&remaining, &parents[p])?;
    }
    Ok(remaining.is_empty())
}

/// `lines` without a longest common subsequence of `lines` and `with`.
fn take_common(lines: &[u32], with: &[u32]) -> Result<Vec<u32>, TooLarge> {
    let head = lines.iter().zip(with).take_while(|(a, b)| a == b).count();
    let tail = lines[head ..]
        .iter()
        .rev()
        .zip(with[head ..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a = &lines[head .. lines.len() - tail];
    let b = &with[head .. with.len() - tail];
    if a.is_empty() || b.is_empty() {
        return Ok(a.to_vec());
    }
    if a.len().saturating_mul(b.len()) > TIME_CAP {
        return Err(TooLarge);
    }
    let mut matched = vec![false; a.len()];
    mark_common(a, b, 0, &mut matched);
    Ok(a.iter()
        .zip(&matched)
        .filter(|(_, m)| !**m)
        .map(|(l, _)| *l)
        .collect())
}

/// The last row of the longest-common-subsequence table of `a` against every
/// prefix of `b`: `row[j]` is the length for `b[.. j]`.
fn lcs_row(a: &[u32], b: impl Iterator<Item = u32> + Clone) -> Vec<u32> {
    let width = b.clone().count() + 1;
    let mut row = vec![0u32; width];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.clone().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if *x == y { diagonal + 1 } else { above.max(row[j]) };
            diagonal = above;
        }
    }
    row
}

/// Hirschberg's split: marks in `matched` (offset by `at`) the entries of `a`
/// that make up one longest common subsequence with `b`, in space linear in the
/// two.
fn mark_common(a: &[u32], b: &[u32], at: usize, matched: &mut [bool]) {
    if a.is_empty() || b.is_empty() {
        return;
    }
    if a.len() == 1 {
        matched[at] = b.contains(&a[0]);
        return;
    }
    let mid = a.len() / 2;
    let upper = lcs_row(&a[.. mid], b.iter().copied());
    let lower_a: Vec<u32> = a[mid ..].iter().rev().copied().collect();
    let lower = lcs_row(&lower_a, b.iter().rev().copied());
    let split = (0 ..= b.len())
        .max_by_key(|j| upper[*j] + lower[b.len() - *j])
        .unwrap_or(0);
    mark_common(&a[.. mid], &b[.. split], at, matched);
    mark_common(&a[mid ..], &b[split ..], at + mid, matched);
}

/// Every ordering of `items[from..]`, handed to `visit`.
fn permute(items: &mut Vec<usize>, from: usize, visit: &mut impl FnMut(&[usize])) {
    if from + 1 >= items.len() {
        visit(items);
        return;
    }
    for i in from .. items.len() {
        items.swap(from, i);
        permute(items, from + 1, visit);
        items.swap(from, i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(s: &str) -> Vec<&[u8]> {
        if s.is_empty() {
            return Vec::new();
        }
        s.split('\n').map(str::as_bytes).collect()
    }

    fn verdict(staged: &str, base: &str, parents: &[&str]) -> Verdict {
        let parents: Vec<Vec<&[u8]>> = parents.iter().map(|p| split(p)).collect();
        recombines(&split(staged), &parents, &split(base))
    }

    fn ok(staged: &str, base: &str, parents: &[&str]) -> bool {
        verdict(staged, base, parents) == Verdict::Recombines
    }

    #[test]
    fn a_blank_line_each_side_brought_may_both_stand() {
        // two appended sections, each with its own separator, over a base with
        // none: the control for the base bound
        assert!(ok(
            "x\n\n## A\nbody\n\n## B\nbody\n",
            "x\n",
            &["x\n\n## A\nbody\n", "x\n\n## B\nbody\n"],
        ));
    }

    #[test]
    fn a_hand_typed_copy_of_a_line_both_parents_carry_once_is_refused_at_either_spot() {
        // the base has the line once and neither side added a second
        let base = "head\nx += 1;\nmid\ntail\n";
        let parents = ["head\nx += 1;\nours\ntail\n", "head\nx += 1;\ntheirs\ntail\n"];
        assert!(ok("head\nx += 1;\nours\ntheirs\ntail\n", base, &parents), "the control");
        assert!(!ok("head\nx += 1;\nx += 1;\nours\ntheirs\ntail\n", base, &parents));
        assert!(!ok("head\nx += 1;\nours\nx += 1;\ntheirs\ntail\n", base, &parents));
    }

    #[test]
    fn a_line_both_sides_added_may_stand_twice_but_not_three_times() {
        // each side added its own copy to a base without it, as git's merge
        // would leave them
        let parents = ["h\nx\n", "h\nx\n"];
        assert!(ok("h\nx\nx\n", "h\n", &parents));
        assert!(!ok("h\nx\nx\nx\n", "h\n", &parents));
    }

    #[test]
    fn an_octopus_is_bounded_by_the_base_and_what_each_parent_added() {
        let parents = ["h\nx\n", "h\nx\n", "h\nx\n"];
        assert!(ok("h\nx\nx\nx\n", "h\n", &parents));
        assert!(!ok("h\nx\nx\nx\nx\n", "h\n", &parents));
        // the base already carried it once, so the parents added nothing
        assert!(!ok("h\nx\nx\n", "h\nx\n", &parents));
    }

    #[test]
    fn with_no_base_the_bound_is_what_the_parents_carry_together() {
        assert!(ok("h\nx\nx\n", "", &["h\nx\n", "h\nx\n"]));
        assert!(!ok("h\nx\nx\nx\n", "", &["h\nx\n", "h\nx\n"]));
    }

    #[test]
    fn more_than_four_parents_are_tried_in_the_given_order_and_its_reverse_only() {
        let first = "x\na\na\nb\nb\n";
        let swapped = "x\nb\nb\na\na\n";
        let five = [first, "y", "y", "y", "y"];
        assert!(!ok(swapped, "", &[first, "y"]));
        assert!(!ok(swapped, "", &five), "no ordering explains the swap, and no fail-open");
        // a consolidation with one line from each parent, in the given order or not
        let each = ["h\n1\n", "h\n2\n", "h\n3\n", "h\n4\n", "h\n5\n"];
        assert!(ok("h\n1\n2\n3\n4\n5\n", "h\n", &each));
        assert!(ok("h\n5\n4\n3\n2\n1\n", "h\n", &each));
        // and a line moved within the one parent that has several is refused
        let two = ["h\n1\n2\n", "h\n3\n", "h\n4\n", "h\n5\n", "h\n6\n"];
        assert!(ok("h\n1\n2\n3\n4\n5\n6\n", "h\n", &two));
        assert!(!ok("h\n2\n1\n3\n4\n5\n6\n", "h\n", &two));
    }

    #[test]
    fn four_parents_try_every_ordering() {
        let each = ["h\n1\n", "h\n2\n", "h\n3\n", "h\n4\n"];
        assert!(ok("h\n3\n1\n4\n2\n", "h\n", &each));
    }

    #[test]
    fn a_table_past_the_time_cap_is_refused_and_not_passed() {
        let side = |tag: &str| -> String { (0 .. 25_000).map(|i| format!("{tag}{i}\n")).collect() };
        let (a, b) = (side("a"), side("b"));
        // the first side reversed puts nothing in the first side's order
        let reversed: String = a.lines().rev().map(|l| format!("{l}\n")).collect();
        assert_eq!(
            verdict(&format!("{reversed}{b}"), "", &[&a, &b]),
            Verdict::TooLarge
        );
    }

    #[test]
    fn a_consolidation_taking_one_sides_order_where_the_sides_disagree_is_inherited() {
        // trunk re-sorted two imports the base had and the branch kept, and the
        // branch added a line. Taking trunk's order and the branch's line is the
        // merge git would make with trunk's hunk; before this it read as
        // authored, since the shared lines' order could equal only one parent.
        let base = "use a;\nuse c;\nbody\n";
        let parents = ["use a;\nuse c;\nbody\nmine\n", "use c;\nuse a;\nbody\n"];
        assert!(ok("use c;\nuse a;\nbody\nmine\n", base, &parents));
        assert!(ok("use a;\nuse c;\nbody\nmine\n", base, &parents), "the branch's order too");
    }

    #[test]
    fn a_swap_of_two_lines_no_parent_swapped_is_still_refused_where_the_sides_disagree_elsewhere() {
        // the negative beside it: the sides disagree on a and c, and both keep
        // x before y, so putting y first is an edit nobody made
        let base = "use a;\nuse c;\nx\ny\n";
        let parents = ["use a;\nuse c;\nx\ny\nmine\n", "use c;\nuse a;\nx\ny\n"];
        assert!(ok("use c;\nuse a;\nx\ny\nmine\n", base, &parents), "the control");
        assert!(!ok("use c;\nuse a;\ny\nx\nmine\n", base, &parents));
    }

    #[test]
    fn where_the_sides_disagree_the_shared_lines_take_one_sides_order_whole_and_never_a_mix() {
        // trunk moved c to the top. Putting c between a and b is a place no
        // side had it, though each pair of the three is in some side's order.
        let base = "a\nb\nc\n";
        let parents = ["a\nb\nc\nmine\n", "c\na\nb\n"];
        assert_eq!(verdict("c\na\nb\nmine\n", base, &parents), Verdict::Recombines, "the control");
        assert_eq!(verdict("a\nc\nb\nmine\n", base, &parents), Verdict::Authored(Why::Order));
        // three parents covering every pair between them, and a reversal none has
        let three = ["a\nb\nc\n", "b\nc\na\n", "c\na\nb\n"];
        assert_eq!(verdict("b\nc\na\n", base, &three), Verdict::Recombines, "the control");
        assert_eq!(verdict("c\nb\na\n", base, &three), Verdict::Authored(Why::Order));
        // each side moved a different shared line: a recombination, refused on
        // purpose, so that widening the rule to a three-way merge of the orders is
        // a choice somebody makes and not a drift
        let moved_apart = ["b\na\nc\nd\n", "a\nb\nd\nc\n"];
        let base4 = "a\nb\nc\nd\n";
        assert_eq!(verdict("b\na\nc\nd\n", base4, &moved_apart), Verdict::Recombines, "the control");
        assert_eq!(verdict("b\na\nd\nc\n", base4, &moved_apart), Verdict::Authored(Why::Order));
    }

    #[test]
    fn a_refusal_names_the_test_that_refused_it() {
        // a merge refused with no reason left a helper unable to tell a typed
        // line from a moved one, so each test answers for itself
        let base = "h\nx\ny\n";
        let parents = ["h\nx\ny\nours\n", "h\nx\ny\ntheirs\n"];
        assert_eq!(verdict("h\nx\ny\nours\ntheirs\n", base, &parents), Verdict::Recombines, "the control");
        assert_eq!(
            verdict("h\nx\ny\nours\ntheirs\nmine\n", base, &parents),
            Verdict::Authored(Why::Counted)
        );
        assert_eq!(verdict("h\ny\nx\nours\ntheirs\n", base, &parents), Verdict::Authored(Why::Order));
        // a and b repeat, so no line anchors them, and only the sequence test sees the swap
        assert_eq!(
            verdict("x\nb\nb\na\na\n", "", &["x\na\na\nb\nb\n", "y"]),
            Verdict::Authored(Why::Sequence)
        );
    }

    #[test]
    fn the_split_longest_common_subsequence_is_one_and_a_longest() {
        // against a plain table, over many small inputs from a fixed generator
        let mut seed = 0x2545_f491_u64;
        let mut next = move |m: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % m
        };
        for _ in 0 .. 400 {
            let a: Vec<u32> = (0 .. next(14)).map(|_| next(4) as u32).collect();
            let b: Vec<u32> = (0 .. next(14)).map(|_| next(4) as u32).collect();
            let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
            for i in 0 .. a.len() {
                for j in 0 .. b.len() {
                    table[i + 1][j + 1] = if a[i] == b[j] {
                        table[i][j] + 1
                    } else {
                        table[i][j + 1].max(table[i + 1][j])
                    };
                }
            }
            let mut matched = vec![false; a.len()];
            mark_common(&a, &b, 0, &mut matched);
            let taken: Vec<u32> = a.iter().zip(&matched).filter(|(_, m)| **m).map(|(x, _)| *x).collect();
            assert_eq!(taken.len(), table[a.len()][b.len()], "{a:?} against {b:?}");
            let mut rest = b.iter();
            assert!(
                taken.iter().all(|x| rest.any(|y| y == x)),
                "{taken:?} is not a subsequence of {b:?}"
            );
        }
    }
}

