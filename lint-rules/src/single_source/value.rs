//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Canonical forms of a constant's type and value, so two spellings of one
//! thing compare equal and two different things never do.
//!
//! A value is compared only when it is *closed*: built from literals and the
//! operators between them. Anything that names something (a path, a call, a
//! struct expression, a macro) is *open*, because two crates spelling the same
//! name can mean two different values, and a lint that gates at error cannot
//! guess. An open value is never reported; see the module doc of the parent for
//! the limit this draws.

use tree_sitter::Node;

/// Integer type suffixes. A hex literal can only carry these, since `f` is a
/// hex digit and would otherwise eat the end of `0x1f32`.
const INT_SUFFIXES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
];

/// Suffixes a non-hex numeric literal may end in: the integers, and the floats,
/// because the grammar lexes `1f32` as an integer literal.
const NUM_SUFFIXES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64",
];

/// Node kinds a closed value may be made of. Each one is closed exactly when all
/// of its named children are.
const COMPOSITE: &[&str] = &[
    "unary_expression",
    "binary_expression",
    "parenthesized_expression",
    "array_expression",
    "tuple_expression",
    "reference_expression",
    "type_cast_expression",
    "negative_literal",
    "unit_expression",
    "mutable_specifier",
];

/// The leaf tokens of a type, joined by one space, so layout does not matter.
pub(super) fn canonical_type(node: Node, source: &str) -> String {
    let mut tokens = Vec::new();
    leaf_tokens(node, source, &mut tokens);
    tokens.join(" ")
}

fn leaf_tokens<'s>(node: Node, source: &'s str, out: &mut Vec<&'s str>) {
    if node.kind().contains("comment") {
        return;
    }
    if node.child_count() == 0 {
        out.push(&source[node.byte_range()]);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        leaf_tokens(child, source, out);
    }
}

/// The canonical form of a closed value, or `None` when it is open.
///
/// A literal that *is* the whole value drops its type suffix, because the
/// constant's own type already says it. A literal anywhere inside an operator,
/// cast, array or tuple keeps its suffix: there it decides what the expression
/// computes (`!0u8 as u32` is 255, `!0u16 as u32` is 65535), and a lint that
/// gates at error cannot call those the same.
pub(super) fn canonical_value(node: Node, source: &str) -> Option<String> {
    value(node, source, true)
}

fn value(node: Node, source: &str, whole: bool) -> Option<String> {
    match node.kind() {
        "integer_literal" | "float_literal" => {
            Some(canonical_number(&source[node.byte_range()], whole))
        },
        "boolean_literal" | "char_literal" | "string_literal" | "raw_string_literal" => {
            Some(source[node.byte_range()].to_string())
        },
        kind if COMPOSITE.contains(&kind) => composite(node, source),
        _ => None,
    }
}

fn composite(node: Node, source: &str) -> Option<String> {
    let cast_type = if node.kind() == "type_cast_expression" {
        node.child_by_field_name("type")
    } else {
        None
    };
    let mut parts: Vec<String> = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind().contains("comment") {
            continue;
        }
        if !child.is_named() {
            parts.push(source[child.byte_range()].to_string());
        } else if cast_type == Some(child) {
            parts.push(canonical_type(child, source));
        } else {
            parts.push(value(child, source, false)?);
        }
    }
    Some(parts.join(" "))
}

/// A numeric literal with its underscores, radix and float spelling removed:
/// `1_000u32`, `0x3E8` and `1000` are one value. The type suffix is kept unless
/// the literal is `whole`, the entire value of the constant.
fn canonical_number(text: &str, whole: bool) -> String {
    let lower: String = text
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_lowercase();
    let radix = [("0x", 16), ("0o", 8), ("0b", 2)]
        .into_iter()
        .find(|(prefix, _)| lower.starts_with(prefix));
    let (suffix, number) = if let Some((prefix, radix)) = radix {
        let rest = &lower[prefix.len() ..];
        let (body, suffix) = split_suffix(rest, INT_SUFFIXES);
        let number = u128::from_str_radix(body, radix)
            .map(|n| n.to_string())
            .unwrap_or_else(|_| lower.clone());
        (suffix, number)
    } else {
        let (body, suffix) = split_suffix(&lower, NUM_SUFFIXES);
        let float = body.contains(['.', 'e']) || suffix.starts_with('f');
        let number = if float {
            body.parse::<f64>().ok().map(|f| format!("{f:?}"))
        } else {
            body.parse::<u128>().ok().map(|n| n.to_string())
        };
        (suffix, number.unwrap_or_else(|| body.to_string()))
    };
    if whole { number } else { format!("{number}{suffix}") }
}

/// The text without a trailing type suffix, and the suffix.
fn split_suffix<'s>(text: &'s str, suffixes: &[&'static str]) -> (&'s str, &'static str) {
    suffixes
        .iter()
        .find_map(|s| text.strip_suffix(s).map(|body| (body, *s)))
        .unwrap_or((text, ""))
}
