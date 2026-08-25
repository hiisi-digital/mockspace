//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

/// Every subcommand `run_inner` dispatches on. Single source of truth for
/// the dispatch match, the unknown-subcommand help, and the suggestion.
/// Every known subcommand.
///
/// Derived from the one table that also carries each summary, in
/// [`super::help`], so a subcommand cannot exist in the dispatch and be missing
/// from help, or be suggestible and undocumented. The previous hand-maintained
/// copy had already drifted: it was missing `check-message`.
pub(crate) fn known_subcommands() -> Vec<&'static str> {
    super::help::known_commands()
}

/// Classic Levenshtein edit distance between two ASCII-ish words. Small
/// inputs (subcommand names), so the simple two-row DP is more than enough.
pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0 ..= b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Nearest known subcommand to `input`, when close enough to be a likely
/// typo. The threshold scales with word length so short words need a close
/// match and longer ones tolerate a little more.
///
/// Case-insensitive, because `LOCK` is not a different intention from `lock`.
/// An input that is a prefix of exactly one command suggests that command
/// whatever the distance says: someone typing `dep` has not misspelled
/// anything, they stopped early, and a distance threshold alone rejects it.
/// A prefix of several commands stays ambiguous and falls through to the
/// distance rule rather than guessing.
pub(crate) fn suggest_subcommand(input: &str) -> Option<&'static str> {
    let input = input.to_ascii_lowercase();

    // Two characters minimum: a single letter is a prefix of something almost
    // by accident, and any pick from it would be arbitrary.
    if input.len() >= 2 {
        let mut prefix_of = known_subcommands()
            .into_iter()
            .filter(|name| name.starts_with(&input));
        if let (Some(only), None) = (prefix_of.next(), prefix_of.next()) {
            return Some(only);
        }
    }

    let mut best: Option<(&'static str, usize)> = None;
    for name in known_subcommands() {
        let d = levenshtein(&input, name);
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((name, d));
        }
    }
    let (name, dist) = best?;
    let threshold = (input.len() / 2).max(2);
    (dist <= threshold).then_some(name)
}
