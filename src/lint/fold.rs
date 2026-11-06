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
        asks_for_all(std::env::var(ALL_VAR).ok().as_deref())
}

/// Whether a value of [`ALL_VAR`] asks for every finding; only `all` does.
pub fn asks_for_all(value: Option<&str>) -> bool {
    value == Some("all")
}

/// A heading and the folded findings under it, or nothing where there are none.
pub fn section(title: &str, findings: &[LintError], shown: Option<usize>) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let mut text = format!("\n--- {title} ({}) ---", findings.len());
    for line in fold(findings, shown) {
        text.push('\n');
        text.push_str(&line);
    }
    Some(text)
}

/// The sections of lint info and lint warnings a run prints, folded unless `all`.
pub fn sections(info: &[LintError], warnings: &[LintError], all: bool) -> Vec<String> {
    let shown = (!all).then_some(SHOWN_PER_LINT);
    [section("lint info", info, shown), section("lint warnings", warnings, shown)].into_iter().flatten().collect()
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
        assert_eq!(out[2], format!("  [a] ... 4 more, {ALL_VAR}=all lists them"));
        assert_eq!(out[3], finding("b", 0).to_string());
        assert_eq!(out[4], finding("b", 1).to_string());
    }

    #[test]
    fn a_run_prints_the_first_three_findings_of_a_lint_unless_all_is_asked() {
        let out = sections(&[], &many("a", 4), false).join("\n");
        assert!(out.contains("m2") && !out.contains("m3") && out.contains("1 more"), "{out}");
        // The control: asking for all prints the fourth and no count.
        let all = sections(&[], &many("a", 4), true).join("\n");
        assert!(all.contains("m3") && !all.contains("more"), "{all}");
    }

    #[test]
    fn only_all_asks_for_every_finding() {
        assert!(asks_for_all(Some("all")));
        assert!(!asks_for_all(None));
        assert!(!asks_for_all(Some("")));
        assert!(!asks_for_all(Some("1")));
    }

    #[test]
    fn a_section_has_its_heading_with_the_full_count_and_the_folded_lines_under_it() {
        let text = section("lint warnings", &many("a", 5), Some(2)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], "--- lint warnings (5) ---");
        assert_eq!(lines.len(), 1 + 1 + 2 + 1, "{text}");
        assert!(lines[4].contains("3 more"), "{text}");
        // The control: the same findings unfolded print every one and no count.
        let all = section("lint warnings", &many("a", 5), None).unwrap();
        assert_eq!(all.lines().count(), 1 + 1 + 5, "{all}");
        assert!(!all.contains("more"));
    }

    #[test]
    fn a_section_without_findings_prints_nothing() {
        assert!(section("lint info", &[], Some(3)).is_none());
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
