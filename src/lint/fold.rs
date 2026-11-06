//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! How many findings of one lint a run prints beneath its heading.
//!
//! A project can carry hundreds of standing warnings that no change touched, and a commit gate that
//! printed every one of them on every run put them in the context of whoever ran it, with the one
//! line that blocked far down the page. So a lint's findings past the first few are one line with
//! a count, and the whole list is `MOCKSPACE_LINT_WARNINGS=all`.

use mockspace_lint_rules::LintError;

/// The findings of a lint printed before the rest are counted.
pub const SHOWN_PER_LINT: usize = 3;

/// The environment variable that prints every finding.
pub const ALL_VAR: &str = "MOCKSPACE_LINT_WARNINGS";

/// Whether the environment asks for every finding.
pub fn wants_all() -> bool {
    std::env::var(ALL_VAR).is_ok_and(|v| v == "all")
}

/// The lines to print for `findings`, in the order the lints first appear: each finding of a lint
/// up to `shown`, then one line saying how many more it has. `shown` of `None` is every finding.
pub fn fold(findings: &[LintError], shown: Option<usize>) -> Vec<String> {
    let mut order: Vec<&'static str> = Vec::new();
    for f in findings {
        if !order.contains(&f.lint_name) {
            order.push(f.lint_name);
        }
    }
    let mut out = Vec::new();
    for lint in order {
        let group: Vec<&LintError> = findings.iter().filter(|f| f.lint_name == lint).collect();
        let keep = shown.map_or(group.len(), |n| n.min(group.len()));
        out.extend(group[.. keep].iter().map(|f| f.to_string()));
        if keep < group.len() {
            out.push(format!(
                "  [{lint}] ... {} more, {ALL_VAR}=all lists them",
                group.len() - keep
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(lint: &'static str, n: usize) -> LintError {
        LintError::warning("c".into(), n, lint, format!("m{n}"))
    }

    fn many(lint: &'static str, n: usize) -> Vec<LintError> {
        (0 .. n).map(|i| finding(lint, i)).collect()
    }

    #[test]
    fn a_lint_past_the_limit_is_its_first_findings_and_a_count() {
        let out = fold(&many("a", 50), Some(SHOWN_PER_LINT));
        assert_eq!(out.len(), SHOWN_PER_LINT + 1);
        assert!(out[SHOWN_PER_LINT].contains("47 more"), "{}", out[SHOWN_PER_LINT]);
        assert!(out[0].contains("m0"));
    }

    #[test]
    fn every_lint_is_counted_on_its_own_and_in_first_seen_order() {
        let mut all = many("a", 5);
        all.extend(many("b", 2));
        all.extend(many("a", 1));
        let out = fold(&all, Some(2));
        // a: 2 shown + a count of 4; b: both, no count
        assert_eq!(out.len(), 2 + 1 + 2);
        assert!(out[2].contains("[a]") && out[2].contains("4 more"));
        assert!(out[3].contains("[b/") || out[3].contains("[b]") || out[3].contains("b"));
    }

    #[test]
    fn none_prints_every_finding_and_no_count() {
        let out = fold(&many("a", 50), None);
        assert_eq!(out.len(), 50);
        assert!(out.iter().all(|l| !l.contains("more")));
    }

    #[test]
    fn a_lint_at_or_under_the_limit_has_no_count_line() {
        assert_eq!(fold(&many("a", SHOWN_PER_LINT), Some(SHOWN_PER_LINT)).len(), SHOWN_PER_LINT);
    }
}
