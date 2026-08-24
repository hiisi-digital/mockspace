//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

#![allow(unused_imports)]

use super::*;

/// Count words in a markdown fragment, ignoring markdown syntax, HTML
/// comments, and frontmatter. A "word" is any run of non-whitespace
/// separated by whitespace, minus tokens that are purely punctuation.
pub(crate) fn count_bookend_words(text: &str) -> usize {
    let mut clean = String::with_capacity(text.len());
    let mut in_html_comment = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !in_html_comment && c == '<' {
            let rest: String = chars.clone().take(3).collect();
            if rest.starts_with("!--") {
                in_html_comment = true;
                continue;
            }
        }
        if in_html_comment {
            if c == '-' {
                let rest: String = chars.clone().take(2).collect();
                if rest.starts_with("->") {
                    chars.next();
                    chars.next();
                    in_html_comment = false;
                }
            }
            continue;
        }
        clean.push(c);
    }
    clean
        .split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .count()
}

/// Validate a consumer-authored preamble/postamble template against the
/// word budget. Prints a cargo::warning on overflow: non-fatal so
/// existing consumers aren't broken, but visible every `cargo mock` run.
pub(crate) fn validate_bookend_size(content: &str, filename: &str) {
    if content.is_empty() {
        return;
    }
    let words = count_bookend_words(content);
    if words > BOOKEND_MAX_WORDS {
        eprintln!(
            "cargo::warning=mockspace: {filename} is {words} words ({} over budget). \
             Bookends are re-stamped on every rule/skill load; tight constant \
             reminders only. Move longer invariants into MAIN.md.tmpl.",
            words - BOOKEND_MAX_WORDS
        );
    }
}

pub(crate) fn format_with_bookends(
    header: &str,
    preamble: &str,
    body: &str,
    postamble: &str,
) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');
    if !preamble.is_empty() {
        out.push('\n');
        out.push_str(preamble);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(body);
    if !postamble.is_empty() {
        out.push('\n');
        out.push_str(postamble);
        out.push('\n');
    }
    out
}
