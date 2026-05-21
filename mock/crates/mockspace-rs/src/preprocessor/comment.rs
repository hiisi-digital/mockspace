//! Comment-form parser for the five canonical directives.
//!
//! Per the design memo at
//! `mock/research/202605220000_canonical-directive-vocabulary.md`,
//! comments are the canonical surface across all languages. Each
//! language's preprocessor knows its comment delimiter; the directive
//! grammar inside the comment is shared.
//!
//! # Grammar (per directive keyword)
//!
//! ```text
//! lint:allow(<name>)                       [reason: "..."]  [tracked: #N]
//! lint:scope-add(<name>, <axis>=<value>)
//! lint:defer(<name>, until: #N)            [reason: "..."]
//! lint:file-disable(<name>)                [reason: "..."]  [tracked: #N]
//! ```
//!
//! Trailing keyed clauses (`reason:`, `tracked:`, `until:`) parse
//! permissively: missing fields surface as `Option<String> = None`
//! on the resulting [`Directive`] variant and are validated downstream
//! by `SuppressionMetaLint`. The parser never panics on ill-formed
//! directives; it skips the comment and continues scanning.
//!
//! # Known limitations
//!
//! - **Escaped quotes inside quoted strings are not supported.** A
//!   directive of the form `lint:allow(x) reason: "say \"hi\""` will
//!   mis-terminate at the first inner `"`. Workaround: keep quoted
//!   reason text simple. Full escape handling is a follow-up; the
//!   common case (no escapes) is what landed source-comments use.
//!
//! # Scope today
//!
//! This module implements parsing only. Integration with
//! [`crate::preprocessor::RustPreprocessor`] (which converts `Allow`
//! records into [`mockspace_core::lint::SuppressionScope`] entries
//! and forwards the other four kinds to follow-up maps per #546)
//! lands in a follow-up slice of #544.

use mockspace_core::lint::{Directive, DirectiveRecord, PropValue, ScopeAxis, Span};

/// Identify, scan, and parse all directive-bearing comments inside
/// `source`. Returns one [`DirectiveRecord`] per recognised directive.
///
/// Recognises three comment delimiter shapes (covers Rust / Zig / TS):
///
/// - `// ...` line comment
/// - `/* ... */` block comment (single-line bodies; multi-line bodies
///   are walked but the directive must fit on a single source line)
/// - `//! ...` and `/// ...` doc-comment lines (treated as `//` for
///   directive purposes since rustdoc renders them as prose)
///
/// `path` is recorded on every emitted [`Span`] so downstream code can
/// reference the directive's location.
pub fn parse_directives(source: &str, path: &str) -> Vec<DirectiveRecord> {
    let mut out = Vec::new();
    for (line_num_0, line) in source.lines().enumerate() {
        let line_num = (line_num_0 as u32) + 1;
        // Walk every `lint:` occurrence on the line; the first match
        // may fail the comment-delimiter check or fail keyword
        // recognition, but a later match on the same line may
        // succeed. Per design memo we don't enforce single-directive-
        // per-line.
        let mut cursor = 0;
        while let Some(rel) = line[cursor..].find("lint:") {
            let directive_start = cursor + rel;
            if is_in_comment_position(line, directive_start) {
                let body = strip_trailing_block_close(&line[directive_start..]);
                let body_byte_len = body.len() as u32;
                if let Some(record) = parse_directive_body(
                    body,
                    path,
                    line_num,
                    directive_start as u32,
                    body_byte_len,
                ) {
                    out.push(record);
                }
            }
            cursor = directive_start + "lint:".len();
        }
    }
    out
}

/// Return true if the text preceding byte `idx` on `line` ends with a
/// recognised comment delimiter. Whitespace between the delimiter and
/// `lint:` is permitted.
fn is_in_comment_position(line: &str, idx: usize) -> bool {
    let prefix = &line[..idx];
    let trimmed = prefix.trim_end();
    trimmed.ends_with("//")
        || trimmed.ends_with("///")
        || trimmed.ends_with("//!")
        || trimmed.ends_with("/*")
        || trimmed.ends_with("#")
        || trimmed.ends_with("<!--")
}

/// Drop a trailing `*/` or `-->` closer so the body parser sees the
/// directive args without comment scaffolding.
fn strip_trailing_block_close(body: &str) -> &str {
    let trimmed = body.trim_end();
    if let Some(rest) = trimmed.strip_suffix("*/") {
        rest.trim_end()
    } else if let Some(rest) = trimmed.strip_suffix("-->") {
        rest.trim_end()
    } else {
        trimmed
    }
}

/// Parse `lint:<keyword>(<args>) [trailing-keyed-clauses]` into a
/// [`DirectiveRecord`]. Returns `None` for ill-formed directives.
///
/// `directive_byte_start` is the zero-indexed byte offset of `l` in
/// `lint:` on the source line. `body_byte_len` is the length of the
/// directive payload (from `l` through the closing `)` plus any
/// trailing keyed clauses), used to compute the emitted [`Span`]'s
/// end column.
fn parse_directive_body(
    body: &str,
    path: &str,
    line: u32,
    directive_byte_start: u32,
    body_byte_len: u32,
) -> Option<DirectiveRecord> {
    let after_marker = body.strip_prefix("lint:")?;
    let paren_open = after_marker.find('(')?;
    let keyword = &after_marker[..paren_open];
    let after_keyword = &after_marker[paren_open + 1..];
    let paren_close = find_matching_paren(after_keyword)?;
    let args = &after_keyword[..paren_close];
    let tail = after_keyword[paren_close + 1..].trim_start();

    // Span columns are 1-indexed; the marker starts at
    // (directive_byte_start + 1).
    let span = Span::single_line(path, line, directive_byte_start + 1, body_byte_len);
    let directive = match keyword {
        "allow" => parse_allow(args, tail)?,
        "scope-add" => parse_scope_add(args)?,
        "defer" => parse_defer(args, tail)?,
        "file-disable" => parse_file_disable(args, tail)?,
        "prop" => parse_prop(args, tail)?,
        _ => return None,
    };
    Some(DirectiveRecord::from_comment(directive, span))
}

/// Find the matching close-paren depth-1 from the start of `s`.
/// Returns the index of the close paren or `None` if unbalanced.
fn find_matching_paren(s: &str) -> Option<usize> {
    let mut depth: i32 = 1;
    let mut in_string = false;
    for (i, ch) in s.char_indices() {
        if in_string {
            if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_allow(args: &str, tail: &str) -> Option<Directive> {
    let lint_name = trim_name(args)?;
    let (reason, tracked) = parse_reason_tracked_tail(tail);
    Some(Directive::Allow {
        lint_name,
        reason,
        tracked,
    })
}

fn parse_scope_add(args: &str) -> Option<Directive> {
    let (lint_name_raw, rest) = args.split_once(',')?;
    let lint_name = trim_name(lint_name_raw)?;
    let (axis_raw, value_raw) = rest.split_once('=')?;
    let axis = parse_scope_axis(axis_raw.trim())?;
    let value = trim_value(value_raw)?;
    Some(Directive::ScopeAdd {
        lint_name,
        axis,
        value,
    })
}

fn parse_defer(args: &str, tail: &str) -> Option<Directive> {
    let (name_raw, rest) = args.split_once(',')?;
    let lint_name = trim_name(name_raw)?;
    let until = parse_keyed_value(rest.trim_start(), "until:")?;
    let (reason, _) = parse_reason_tracked_tail(tail);
    Some(Directive::Defer {
        lint_name,
        until,
        reason,
    })
}

fn parse_file_disable(args: &str, tail: &str) -> Option<Directive> {
    let lint_name = trim_name(args)?;
    let (reason, tracked) = parse_reason_tracked_tail(tail);
    Some(Directive::FileDisable {
        lint_name,
        reason,
        tracked,
    })
}

/// Parse `lint:prop(<name>)` or `lint:prop(<name> = <value>)` per the
/// design memo at
/// `mock/research/202605220600_lint-provided-marker-directive.md`.
///
/// Forms accepted inside the parens:
/// - `<name>`                  → `PropValue::Bool(true)` (presence)
/// - `<name> = true|false`     → `PropValue::Bool(...)`
/// - `<name> = <integer>`      → `PropValue::Integer(...)`
/// - `<name> = "<string>"`     → `PropValue::String(...)`
///
/// Trailing `reason: "..."` clause attaches as on the other directives.
fn parse_prop(args: &str, tail: &str) -> Option<Directive> {
    let (name, value) = match args.split_once('=') {
        Some((name_raw, value_raw)) => {
            let name = trim_name(name_raw)?;
            let value = parse_prop_value(value_raw.trim())?;
            (name, value)
        }
        None => {
            let name = trim_name(args)?;
            (name, PropValue::Bool(true))
        }
    };
    let (reason, _) = parse_reason_tracked_tail(tail);
    Some(Directive::Prop {
        name,
        value,
        reason,
    })
}

/// Parse a [`PropValue`] from the right-hand side of a key-value prop
/// directive. Accepts `true` / `false`, bare integer literals (decimal,
/// optionally signed), and quoted string literals (`"..."`).
fn parse_prop_value(s: &str) -> Option<PropValue> {
    let s = s.trim();
    if s == "true" {
        return Some(PropValue::Bool(true));
    }
    if s == "false" {
        return Some(PropValue::Bool(false));
    }
    if let Some(rest) = s.strip_prefix('"') {
        let close = rest.find('"')?;
        return Some(PropValue::String(rest[..close].to_string()));
    }
    // Bare integer: optionally signed, decimal digits only. The bare
    // numeric form lets authors write `arena_size = 4096` without
    // quotes; signed for symmetry with the underlying i64 PropValue.
    if let Ok(n) = s.parse::<i64>() {
        return Some(PropValue::Integer(n));
    }
    None
}

/// Trim a single bare token (lint-name or category-name) from `s`.
/// Names match `[a-zA-Z_][a-zA-Z0-9_-]*` (kebab-case identifiers).
fn trim_name(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let valid = t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return None;
    }
    if !t
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '_')
        .unwrap_or(false)
    {
        return None;
    }
    Some(t.to_string())
}

/// Trim a value: either a quoted string or a bare identifier.
fn trim_value(s: &str) -> Option<String> {
    let t = s.trim();
    if let Some(stripped) = t.strip_prefix('"') {
        return stripped.strip_suffix('"').map(|s| s.to_string());
    }
    trim_name(t)
}

fn parse_scope_axis(s: &str) -> Option<ScopeAxis> {
    Some(match s {
        "paths" => ScopeAxis::Paths,
        "exempt_paths" => ScopeAxis::ExemptPaths,
        "crates" => ScopeAxis::Crates,
        "exempt_crates" => ScopeAxis::ExemptCrates,
        "languages" => ScopeAxis::Languages,
        "proc_macro_exempt" => ScopeAxis::ProcMacroExempt,
        _ => return None,
    })
}

/// Parse `<key>: <value>` returning the value as a String. Accepts
/// quoted strings (`"text"`) and bare tokens (`#427`, identifiers).
///
/// Quoted regions in `text` are skipped during the key lookup, so a
/// substring match inside `reason: "see tracked: #5"` does not return
/// `#5"` as a tracked value.
fn parse_keyed_value(text: &str, key: &str) -> Option<String> {
    let start = find_key_outside_strings(text, key)?;
    let after = &text[start + key.len()..];
    let after = after.trim_start();
    if let Some(rest) = after.strip_prefix('"') {
        let close = rest.find('"')?;
        Some(rest[..close].to_string())
    } else {
        // Bare token: read until whitespace or known delimiter.
        let end = after
            .find(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or(after.len());
        let value = after[..end].trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }
}

/// Find `key` in `text` ignoring matches inside double-quoted regions.
/// Returns the byte offset of the first occurrence outside any quoted
/// region, or `None` if the key never appears outside strings.
fn find_key_outside_strings(text: &str, key: &str) -> Option<usize> {
    let mut in_string = false;
    let bytes = text.as_bytes();
    let key_bytes = key.as_bytes();
    let mut i = 0;
    while i + key_bytes.len() <= bytes.len() {
        let ch = bytes[i];
        if ch == b'"' {
            in_string = !in_string;
            i += 1;
            continue;
        }
        if !in_string && &bytes[i..i + key_bytes.len()] == key_bytes {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Walk the trailing portion of a directive line for `reason:` and
/// `tracked:` clauses. Both are optional; absent fields surface as
/// `None`.
fn parse_reason_tracked_tail(tail: &str) -> (Option<String>, Option<String>) {
    let reason = parse_keyed_value(tail, "reason:");
    let tracked = parse_keyed_value(tail, "tracked:");
    (reason, tracked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::Directive;

    #[test]
    fn parses_lint_allow_with_reason_and_tracked() {
        let src = r#"
// lint:allow(no-bare-numeric) reason: "spec-fixed constant" tracked: #427
const FNV_PRIME: u64 = 0x100000001b3;
"#;
        let recs = parse_directives(src, "a.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Allow {
                lint_name,
                reason,
                tracked,
            } => {
                assert_eq!(lint_name, "no-bare-numeric");
                assert_eq!(reason.as_deref(), Some("spec-fixed constant"));
                assert_eq!(tracked.as_deref(), Some("#427"));
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn parses_lint_scope_add_with_axis_value() {
        let src = "// lint:scope-add(no-bare-numeric, exempt_paths=\"tests/**\")\nmod ffi {}\n";
        let recs = parse_directives(src, "m.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::ScopeAdd {
                lint_name,
                axis,
                value,
            } => {
                assert_eq!(lint_name, "no-bare-numeric");
                assert_eq!(*axis, ScopeAxis::ExemptPaths);
                assert_eq!(value, "tests/**");
            }
            other => panic!("expected ScopeAdd, got {other:?}"),
        }
    }

    #[test]
    fn parses_lint_scope_add_all_six_axes() {
        let cases = [
            ("paths", ScopeAxis::Paths),
            ("exempt_paths", ScopeAxis::ExemptPaths),
            ("crates", ScopeAxis::Crates),
            ("exempt_crates", ScopeAxis::ExemptCrates),
            ("languages", ScopeAxis::Languages),
            ("proc_macro_exempt", ScopeAxis::ProcMacroExempt),
        ];
        for (axis_str, expected) in cases {
            let src = format!("// lint:scope-add(my-lint, {axis_str}=value)\n");
            let recs = parse_directives(&src, "x.rs");
            assert_eq!(recs.len(), 1, "axis `{axis_str}` did not parse");
            assert!(
                matches!(&recs[0].directive, Directive::ScopeAdd { axis, .. } if *axis == expected),
                "axis `{axis_str}` produced wrong variant",
            );
        }
    }

    #[test]
    fn parses_lint_defer_with_until_and_reason() {
        let src = r#"// lint:defer(no-bare-string, until: #185) reason: "clause test rehab pending"
fn legacy(name: String) {}
"#;
        let recs = parse_directives(src, "t.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Defer {
                lint_name,
                until,
                reason,
            } => {
                assert_eq!(lint_name, "no-bare-string");
                assert_eq!(until, "#185");
                assert_eq!(reason.as_deref(), Some("clause test rehab pending"));
            }
            other => panic!("expected Defer, got {other:?}"),
        }
    }

    #[test]
    fn parses_lint_file_disable_with_reason_and_tracked() {
        let src = r#"// lint:file-disable(writing-style) reason: "generated FFI bindings" tracked: #207
"#;
        let recs = parse_directives(src, "gen.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::FileDisable {
                lint_name,
                reason,
                tracked,
            } => {
                assert_eq!(lint_name, "writing-style");
                assert_eq!(reason.as_deref(), Some("generated FFI bindings"));
                assert_eq!(tracked.as_deref(), Some("#207"));
            }
            other => panic!("expected FileDisable, got {other:?}"),
        }
    }

    #[test]
    fn missing_reason_and_tracked_yield_none_not_panic() {
        let src = "// lint:allow(no-bare-numeric)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Allow {
                reason, tracked, ..
            } => {
                assert!(reason.is_none());
                assert!(tracked.is_none());
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn block_comment_form_is_recognised() {
        let src = "/* lint:allow(no-bare-numeric) reason: \"x\" tracked: #1 */\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        assert!(matches!(&recs[0].directive, Directive::Allow { .. }));
    }

    #[test]
    fn doc_comment_forms_are_recognised() {
        let src = "/// lint:allow(no-bare-numeric)\n//! lint:allow(no-bare-string)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 2);
    }

    #[test]
    fn toml_hash_form_is_recognised() {
        let src = "# lint:allow(no-bare-numeric) reason: \"toml file\"\n";
        let recs = parse_directives(src, "Cargo.toml");
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn markdown_html_comment_form_is_recognised() {
        let src =
            "<!-- lint:file-disable(writing-style) reason: \"generated table\" tracked: #x -->\n";
        let recs = parse_directives(src, "README.md");
        assert_eq!(recs.len(), 1);
        assert!(matches!(&recs[0].directive, Directive::FileDisable { .. }));
    }

    #[test]
    fn unknown_directive_keyword_is_skipped() {
        let src = "// lint:nonexistent(foo)\n";
        let recs = parse_directives(src, "x.rs");
        assert!(recs.is_empty());
    }

    #[test]
    fn unbalanced_paren_is_skipped() {
        let src = "// lint:allow(no-bare-numeric\nconst x: u32 = 1;\n";
        let recs = parse_directives(src, "x.rs");
        assert!(recs.is_empty());
    }

    #[test]
    fn lint_not_in_comment_position_is_ignored() {
        let src = "let s = \"lint:allow(foo)\";\n";
        let recs = parse_directives(src, "x.rs");
        assert!(recs.is_empty());
    }

    #[test]
    fn span_records_the_directive_position() {
        let src = "fn x() {}\n    // lint:allow(no-bare-numeric)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        let span = &recs[0].span;
        assert_eq!(span.start_line, 2);
        // The marker `lint:` starts at byte 7 zero-indexed (column 8
        // one-indexed) on the second line.
        assert!(
            span.start_column >= 7,
            "got start_column {}",
            span.start_column,
        );
    }

    #[test]
    fn reason_substring_does_not_steal_tracked_key() {
        // Reviewer #43 finding 2: parse_keyed_value used to walk
        // substrings unconditionally; a quoted reason containing the
        // text `tracked: #5` would mis-attribute that as the tracked
        // value. The find-outside-strings pass should prevent this.
        let src = r#"// lint:allow(no-bare-numeric) reason: "see tracked: #5 for context"
"#;
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Allow {
                reason, tracked, ..
            } => {
                assert_eq!(reason.as_deref(), Some("see tracked: #5 for context"));
                // tracked: appears only inside the reason string, so
                // the directive's tracked field should be None.
                assert!(tracked.is_none(), "got tracked: {tracked:?}");
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn second_lint_marker_on_line_is_parsed_when_first_fails() {
        // Reviewer #43 finding 5: the initial scanner only matched the
        // first `lint:` on a line. If that match was outside a comment
        // (or a different keyword), a later valid directive on the same
        // line was silently dropped.
        let src = "let s = \"lint:bogus(foo)\"; // lint:allow(no-bare-numeric)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        assert!(matches!(&recs[0].directive, Directive::Allow { .. }));
    }

    #[test]
    fn span_covers_directive_payload_through_closing_paren() {
        // Reviewer #43 finding 4: Span shape must point at lint: start
        // with length spanning at least to the closing `)`.
        let src = "    // lint:allow(no-bare-numeric)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        let span = &recs[0].span;
        assert_eq!(span.start_line, 1);
        // `lint:` starts at byte 7 zero-indexed (after "    // "), so
        // start_column = 8 one-indexed.
        assert_eq!(
            span.start_column, 8,
            "got start_column {}",
            span.start_column
        );
        // The directive payload is `lint:allow(no-bare-numeric)` which
        // is 27 bytes; end_column should be at least start + 27.
        assert!(
            span.end_column >= span.start_column + 27,
            "end_column {} should cover the directive (start {}, +27)",
            span.end_column,
            span.start_column,
        );
    }

    #[test]
    fn multiple_directives_in_one_file() {
        let src = "// lint:defer(no-bare-vec, until: #99)\n// lint:allow(no-bare-numeric)\n// lint:file-disable(writing-style)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 3);
        assert!(matches!(&recs[0].directive, Directive::Defer { .. }));
        assert!(matches!(&recs[1].directive, Directive::Allow { .. }));
        assert!(matches!(&recs[2].directive, Directive::FileDisable { .. }));
    }

    #[test]
    fn parses_prop_presence_form() {
        let src = "// lint:prop(audited)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop {
                name,
                value,
                reason,
            } => {
                assert_eq!(name, "audited");
                assert_eq!(*value, PropValue::Bool(true));
                assert!(reason.is_none());
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_prop_integer_keyvalue_form() {
        let src = "// lint:prop(arena_size = 4096)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "arena_size");
                assert_eq!(*value, PropValue::Integer(4096));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_prop_string_keyvalue_form() {
        let src = "// lint:prop(audit_id = \"A-2026-04\")\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "audit_id");
                assert_eq!(*value, PropValue::String("A-2026-04".to_string()));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_prop_bool_keyvalue_form() {
        let src = "// lint:prop(enabled = false)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { name, value, .. } => {
                assert_eq!(name, "enabled");
                assert_eq!(*value, PropValue::Bool(false));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_prop_with_reason() {
        let src = "// lint:prop(audited) reason: \"audit pass 2026-04\"\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { reason, .. } => {
                assert_eq!(reason.as_deref(), Some("audit pass 2026-04"));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn parses_signed_integer_prop_value() {
        let src = "// lint:prop(temperature = -42)\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 1);
        match &recs[0].directive {
            Directive::Prop { value, .. } => {
                assert_eq!(*value, PropValue::Integer(-42));
            }
            other => panic!("expected Prop, got {other:?}"),
        }
    }

    #[test]
    fn prop_with_unrecognised_value_kind_is_skipped() {
        // Bare identifier on the RHS of `=` is not a valid PropValue
        // (only true/false, integers, or quoted strings). The whole
        // directive is dropped, not silently coerced to a string.
        let src = "// lint:prop(foo = bar)\n";
        let recs = parse_directives(src, "x.rs");
        assert!(recs.is_empty(), "got {recs:?}");
    }

    #[test]
    fn multiple_prop_directives_on_same_item_accumulate() {
        // Per the memo: multi-value props write multiple directives;
        // each parses to its own DirectiveRecord. PropMap (slice 3)
        // accumulates them under all_named("...").
        let src =
            "// lint:prop(allowed_import = \"alloc\")\n// lint:prop(allowed_import = \"core\")\n";
        let recs = parse_directives(src, "x.rs");
        assert_eq!(recs.len(), 2);
        for rec in &recs {
            match &rec.directive {
                Directive::Prop { name, .. } => assert_eq!(name, "allowed_import"),
                other => panic!("expected Prop, got {other:?}"),
            }
        }
    }
}
