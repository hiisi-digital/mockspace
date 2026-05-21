//! Source-stripping utilities.
//!
//! [`MockspaceDocument::source_stripped`] caches per-`StripOpts` views over
//! a document. The strip removes selected lexical noise (strings, comments,
//! doc comments, markdown code fences) so token-level primitives can scan
//! the substantive content without false positives on text inside literals.
//!
//! Today the strip is a hand-rolled scanner that handles the common Rust
//! syntax: line comments (`//`, `///`, `//!`), block comments (`/* */`,
//! `/** */`, `/*! */`), and string literals (`"..."`, `r"..."`, byte
//! strings, raw strings with `#`-delimiters). It is not a full lexer; it
//! is correct enough to defeat 99% of false-positive triggers without
//! costing a full parse on every scan. The remaining 1% is fine: the
//! lint emits a finding, the consumer adds a `lint:allow(...)`, the
//! suppression mechanism handles it.

use std::hash::Hash;

/// Options controlling [`strip`].
///
/// Implementations of `Hash`/`Eq` so [`MockspaceDocument`] can key the
/// cache on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StripOpts {
    /// Replace string literal contents with spaces. The surrounding quotes
    /// and `r#"..."#` raw-string sigils stay; only the body becomes blanks.
    pub strings: bool,

    /// Replace line and block comments with spaces. Doc comments are
    /// separate: see `doc_comments`.
    pub comments: bool,

    /// Replace `///` line and `/** */` block doc comments with spaces.
    /// Also covers `//!` and `/*! */` inner doc forms.
    pub doc_comments: bool,

    /// For markdown documents, replace fenced code blocks (``` ... ```)
    /// with spaces. No effect on Rust documents.
    pub code_fences: bool,
}

/// Return a stripped view of `source` per `opts`. Length-preserving:
/// every removed byte becomes a space, so column / line positions in the
/// output match the input. This is load-bearing for span reporting:
/// primitives scan the stripped view, emit findings against original
/// positions, and the positions point at real source.
pub fn strip(source: &str, opts: StripOpts) -> String {
    if !opts.strings && !opts.comments && !opts.doc_comments && !opts.code_fences {
        return source.to_string();
    }
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];

        // Line comment, possibly a doc comment.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let is_doc = i + 2 < bytes.len() && (bytes[i + 2] == b'/' || bytes[i + 2] == b'!');
            let strip_this = (is_doc && opts.doc_comments) || (!is_doc && opts.comments);
            let end = find_line_end(bytes, i);
            if strip_this {
                blank_range(&mut out, i, end);
            }
            i = end;
            continue;
        }

        // Block comment, possibly a doc block.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let is_doc = i + 2 < bytes.len() && (bytes[i + 2] == b'*' || bytes[i + 2] == b'!');
            let strip_this = (is_doc && opts.doc_comments) || (!is_doc && opts.comments);
            let end = find_block_comment_end(bytes, i);
            if strip_this {
                blank_range(&mut out, i, end);
            }
            i = end;
            continue;
        }

        // Raw string. Form: r#"..."#, r##"..."##, etc.
        if b == b'r' || (b == b'b' && i + 1 < bytes.len() && bytes[i + 1] == b'r') {
            if let Some((start, end)) = match_raw_string(bytes, i) {
                if opts.strings {
                    blank_range(
                        &mut out,
                        start + raw_string_open_len(bytes, start),
                        end - raw_string_close_len(bytes, start),
                    );
                }
                i = end;
                continue;
            }
        }

        // Plain string literal: "..." with backslash escapes.
        if b == b'"' {
            let end = find_string_end(bytes, i);
            if opts.strings {
                // Strip between the surrounding quotes.
                blank_range(&mut out, i + 1, end.saturating_sub(1));
            }
            i = end;
            continue;
        }

        // Markdown fenced block: ``` ... ```.
        if opts.code_fences && b == b'`' && is_code_fence_start(bytes, i) {
            let end = find_code_fence_end(bytes, i);
            blank_range(&mut out, i, end);
            i = end;
            continue;
        }

        i += 1;
    }
    String::from_utf8(out)
        .expect("strip preserved length and only blanked bytes; UTF-8 boundaries survive")
}

fn find_line_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() {
        i + 1
    } else {
        i
    }
}

fn find_block_comment_end(bytes: &[u8], from: usize) -> usize {
    // from points at `/*`. Walk past it then look for `*/`.
    let mut i = from + 2;
    let mut depth = 1usize;
    while i + 1 < bytes.len() && depth > 0 {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

fn find_string_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn match_raw_string(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    // r#"..."# or br#"..."# with N `#` characters.
    let mut i = from;
    if bytes[i] == b'b' {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'r' {
        return None;
    }
    i += 1;
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'"' {
        return None;
    }
    let body_start = i + 1;
    let mut j = body_start;
    while j < bytes.len() {
        if bytes[j] == b'"' {
            // Check for `"###` matching opening hashes.
            let mut closing_hashes = 0;
            let mut k = j + 1;
            while k < bytes.len() && bytes[k] == b'#' && closing_hashes < hashes {
                closing_hashes += 1;
                k += 1;
            }
            if closing_hashes == hashes {
                return Some((from, k));
            }
        }
        j += 1;
    }
    Some((from, bytes.len()))
}

fn raw_string_open_len(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if bytes[i] == b'b' {
        i += 1;
    }
    i += 1; // r
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    i += 1; // opening "
    i - start
}

fn raw_string_close_len(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if bytes[i] == b'b' {
        i += 1;
    }
    i += 1; // r
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    hashes + 1 // closing " plus matching hashes
}

fn is_code_fence_start(bytes: &[u8], i: usize) -> bool {
    i + 2 < bytes.len() && bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`'
}

fn find_code_fence_end(bytes: &[u8], from: usize) -> usize {
    // Skip the opening ```.
    let mut i = from + 3;
    while i + 2 < bytes.len() {
        if bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            return i + 3;
        }
        i += 1;
    }
    bytes.len()
}

fn blank_range(out: &mut [u8], start: usize, end: usize) {
    let len = out.len();
    let clamped_end = end.min(len);
    if start >= clamped_end {
        return;
    }
    for byte in &mut out[start..clamped_end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_line_comments() {
        let opts = StripOpts {
            comments: true,
            ..Default::default()
        };
        let out = strip("x // comment\ny", opts);
        assert_eq!(out, "x           \ny");
    }

    #[test]
    fn strips_block_comments() {
        let opts = StripOpts {
            comments: true,
            ..Default::default()
        };
        let out = strip("x /* abc */ y", opts);
        assert_eq!(out, "x           y");
    }

    #[test]
    fn keeps_doc_comments_when_only_comments_set() {
        let opts = StripOpts {
            comments: true,
            ..Default::default()
        };
        let out = strip("/// doc\nx", opts);
        assert_eq!(out, "/// doc\nx");
    }

    #[test]
    fn strips_doc_comments_when_flagged() {
        let opts = StripOpts {
            doc_comments: true,
            ..Default::default()
        };
        let out = strip("/// doc\nx", opts);
        assert_eq!(out, "       \nx");
    }

    #[test]
    fn strips_string_literals_keeping_quotes() {
        let opts = StripOpts {
            strings: true,
            ..Default::default()
        };
        let out = strip("let s = \"hello\";", opts);
        assert_eq!(out, "let s = \"     \";");
    }

    #[test]
    fn handles_escaped_quotes_in_strings() {
        let opts = StripOpts {
            strings: true,
            ..Default::default()
        };
        let out = strip("\"a\\\"b\"", opts);
        assert_eq!(out, "\"    \"");
    }

    #[test]
    fn strips_raw_string() {
        let opts = StripOpts {
            strings: true,
            ..Default::default()
        };
        let out = strip("let s = r#\"abc\"#;", opts);
        assert_eq!(out, "let s = r#\"   \"#;");
    }

    #[test]
    fn preserves_line_count() {
        let opts = StripOpts {
            comments: true,
            strings: true,
            ..Default::default()
        };
        let src = "// a\n\"b\"\nc";
        let out = strip(src, opts);
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
    }
}
