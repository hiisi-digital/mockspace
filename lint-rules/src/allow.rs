//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `lint:allow(<name>)`, honoured by the runner rather than by each lint.
//!
//! A finding is dropped when the line it names carries a marker for its
//! lint, or when the plain comment lines directly above that line do. The
//! second form is what survives rustfmt: a marker written at the end of a
//! function signature is moved off it the moment the line is too long, onto
//! its own line inside the body, where a same-line check no longer sees it
//! and the lint fires on code somebody deliberately allowed. A marker above
//! the item stays where it was put.
//!
//! Doing this here means a lint written without knowing markers exist still
//! respects them, and no lint carries its own copy of the parse. A lint may
//! still check a marker itself to skip work early; the runner's check is the
//! one that decides.

use crate::{LintError, line_lint_allowed};

/// Whether the finding on line `row`, zero-based, is allowed: by a marker on
/// that line, by one on the plain `//` comment lines directly above the item
/// the line belongs to, or, where the line opens a block, by one on the plain
/// comment lines directly below it. The last is the exact shape rustfmt
/// leaves: a marker written after a signature's `{` lands as the body's
/// first line. The item's own line is reached by passing over the wrapped
/// lines of its signature, so one marker above `pub fn` covers every
/// parameter of it. A doc comment, an attribute, a blank line or a finished
/// statement ends either walk, so a marker cannot reach past the item it
/// sits beside.
#[must_use]
pub fn allowed_at(source: &str, row: usize, rule_name: &str) -> bool {
    let lines: Vec<&str> = source.lines().collect();
    let Some(line) = lines.get(row) else {
        return false;
    };
    if line_lint_allowed(line, rule_name) {
        return true;
    }
    let plain_comment = |l: &str| {
        let l = l.trim_start();
        l.starts_with("//") && !l.starts_with("///") && !l.starts_with("//!")
    };
    // A line that neither ends a statement nor opens or closes a block is a
    // continuation of the item above it: a parameter of a wrapped signature,
    // a where clause, a generic list. The walk passes over those to reach the
    // item's own line, so a marker above `pub fn f(` covers `x: &str,` three
    // lines down. A blank line, an attribute or a doc comment is not a
    // continuation.
    let continuation = |l: &str| {
        let t = l.trim();
        !t.is_empty()
            && !t.starts_with('#')
            && !t.starts_with("//")
            && !t.ends_with(';')
            && !t.ends_with('{')
            && !t.ends_with('}')
    };
    let signature_start = lines[.. row]
        .iter()
        .rposition(|l| !continuation(l))
        .map_or(0, |i| i + 1);
    let above = lines[.. signature_start]
        .iter()
        .rev()
        .take_while(|l| plain_comment(l));
    if above.into_iter().any(|l| line_lint_allowed(l, rule_name)) {
        return true;
    }
    if line.trim_end().ends_with('{') {
        let below = lines[row + 1 ..].iter().take_while(|l| plain_comment(l));
        if below.into_iter().any(|l| line_lint_allowed(l, rule_name)) {
            return true;
        }
    }
    false
}

/// The findings of one lint with the allowed ones dropped. `source_of`
/// answers the text a finding's line is in, or `None` where the runner has
/// none, in which case the finding stands: a marker nobody can see allows
/// nothing.
#[must_use]
pub fn honour_allows<'s>(
    errors: Vec<LintError>,
    lint_name: &str,
    source_of: impl Fn(&LintError) -> Option<&'s str>,
) -> Vec<LintError> {
    errors
        .into_iter()
        .filter(|err| {
            let Some(source) = source_of(err) else {
                return true;
            };
            let row = err.line.saturating_sub(1);
            !allowed_at(source, row, lint_name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_on_the_line_or_on_the_comment_lines_above_allows_the_finding() {
        let src = "fn a() {}\n// lint:allow(x) reason: r\n// and more\nfn b() {}\nfn c() {} // lint:allow(x)\n";
        assert!(
            !allowed_at(src, 0, "x"),
            "the control: nothing allows line 1"
        );
        assert!(
            allowed_at(src, 3, "x"),
            "the comment lines above line 4 allow it"
        );
        assert!(allowed_at(src, 4, "x"), "the marker on line 5 allows it");
        assert!(
            !allowed_at(src, 3, "y"),
            "another lint's name allows nothing"
        );
        assert!(!allowed_at(src, 9, "x"), "past the end is not allowed");
    }

    #[test]
    fn a_doc_comment_an_attribute_or_code_above_ends_the_walk() {
        assert!(!allowed_at(
            "// lint:allow(x)\n/// doc\nfn b() {}\n",
            2,
            "x"
        ));
        assert!(!allowed_at(
            "// lint:allow(x)\n#[inline]\nfn b() {}\n",
            2,
            "x"
        ));
        assert!(!allowed_at(
            "// lint:allow(x)\nfn a() {}\nfn b() {}\n",
            2,
            "x"
        ));
        assert!(allowed_at(
            "// lint:allow(x)\n// plain\n// plain\nfn b() {}\n",
            3,
            "x"
        ));
    }

    #[test]
    fn a_marker_rustfmt_moved_into_the_body_still_allows_the_signature() {
        // the exact shape rustfmt leaves: the marker written after `{` lands
        // as the body's first line
        let src =
            "/// doc\npub fn b(s: &str) -> &str {\n    // lint:allow(x) reason: r\n    s\n}\n";
        assert!(allowed_at(src, 1, "x"));
        // and only where the line opens a block, so a marker in a body does
        // not reach the statement above it
        assert!(!allowed_at(
            "let a = 1;\n// lint:allow(x)\nlet b = 2;\n",
            0,
            "x"
        ));
        // and only through plain comments
        assert!(!allowed_at(
            "fn b() {\n    let a = 1;\n    // lint:allow(x)\n}\n",
            0,
            "x"
        ));
    }

    #[test]
    fn a_marker_above_an_item_covers_its_wrapped_signature() {
        let src = "/// doc\n// lint:allow(x) reason: r\npub fn f<'a>(\n    a: &'a str,\n    b: &'a str,\n) -> &'a str {\n    a\n}\n";
        assert!(allowed_at(src, 2, "x"), "the item's own line");
        assert!(allowed_at(src, 3, "x"), "a parameter line");
        assert!(allowed_at(src, 4, "x"), "the last parameter line");
        assert!(
            allowed_at(src, 5, "x"),
            "the return type line, which opens the block"
        );
        assert!(!allowed_at(src, 6, "x"), "the body is not the signature");
        // a statement above is not a continuation, so a marker over it does
        // not leak downward past it
        let src = "// lint:allow(x)\nlet a = 1;\nlet b: &str = \"\";\n";
        assert!(!allowed_at(src, 2, "x"));
        // nor does a blank line carry the reach
        let src = "// lint:allow(x)\n\nfn f(a: &str) {}\n";
        assert!(!allowed_at(src, 2, "x"));
    }

    fn finding(line: usize, path: Option<&str>) -> LintError {
        let mut e = LintError::error("c".to_string(), line, "x", "m".to_string());
        e.path = path.map(str::to_string);
        e
    }

    #[test]
    fn the_runner_drops_an_allowed_finding_and_keeps_one_it_cannot_see() {
        let src = "// lint:allow(x) reason: r\nfn b() {}\nfn c() {}\n";
        let errors = vec![finding(2, None), finding(3, None), finding(2, Some("src/other.rs"))];
        let kept = honour_allows(errors, "x", |e| {
            match e.path.as_deref() {
                None => Some(src),
                Some(_) => None,
            }
        });
        let lines: Vec<(usize, Option<&str>)> =
            kept.iter().map(|e| (e.line, e.path.as_deref())).collect();
        assert_eq!(lines, [(3, None), (2, Some("src/other.rs"))]);
    }
}
