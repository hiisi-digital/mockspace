//! Lint: `#[repr(C)]` structs must not contain ABI-unsafe fields.
//!
//! Vec, String, Box, HashMap, and other dynamically-sized types are not
//! FFI-safe. Placing them inside a `#[repr(C)]` struct is unsound across
//! ABI boundaries regardless of usage patterns.
//!
//! This lint catches the case that `no-bare-vec` misses: a Vec inside a
//! repr(C) struct that has a `lint:allow(bare_collection)` suppression.
//! The bare_collection allow might be justified for non-FFI code, but
//! repr(C) makes it an ABI safety issue that cannot be suppressed.
//!
//! Severity: error on all gates (commit, build, push). No exceptions.

use tree_sitter::Node;

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "repr-c-abi-safety";

/// Types that are not ABI-safe inside `#[repr(C)]` structs.
const ABI_UNSAFE_TYPES: &[(&str, &str)] = &[
    ("Vec", "use a raw pointer + length, or a fixed-size array"),
    ("String", "use *const c_char or a fixed-size byte array"),
    ("Box", "use a raw pointer"),
    ("HashMap", "use a raw pointer to an opaque type"),
    ("HashSet", "use a raw pointer to an opaque type"),
    ("BTreeMap", "use a raw pointer to an opaque type"),
    ("BTreeSet", "use a raw pointer to an opaque type"),
    ("Arc", "use a raw pointer"),
    ("Rc", "use a raw pointer"),
    ("Cow", "use a concrete owned or borrowed type"),
    ("VecDeque", "use a raw pointer + length"),
    ("LinkedList", "use a raw pointer"),
    ("BinaryHeap", "use a raw pointer"),
    // storage crate types (also dynamically-sized)
    ("Collection", "use a raw pointer + length"),
    ("Dictionary", "use a raw pointer to an opaque type"),
    ("DenseColumn", "use a raw pointer + length"),
    ("IdMap", "use a raw pointer to an opaque type"),
    ("SparseArray", "use a raw pointer to an opaque type"),
    ("SparseSet", "use a raw pointer to an opaque type"),
    ("ErasedArena", "use a raw pointer to an opaque type"),
];

pub struct ReprCAbiSafety;

impl Lint for ReprCAbiSafety {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if crate::PROC_MACRO_CRATES.contains(&ctx.crate_name) {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let root = ctx.tree.root_node();
        visit_structs(root, ctx, &mut errors);
        errors
    }
}

fn visit_structs(node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "struct_item" {
            if has_repr_c(child, ctx.source) {
                check_fields(child, ctx, errors);
            }
        }
        if child.named_child_count() > 0 {
            visit_structs(child, ctx, errors);
        }
    }
}

/// Check whether a struct has `#[repr(C)]`.
fn has_repr_c(struct_node: Node, source: &str) -> bool {
    // Check preceding siblings for attribute_item containing repr(C)
    if let Some(parent) = struct_node.parent() {
        let mut sibling_cursor = parent.walk();
        let mut last_attr_is_repr_c = false;
        for child in parent.children(&mut sibling_cursor) {
            if child.id() == struct_node.id() {
                return last_attr_is_repr_c;
            }
            if child.kind() == "attribute_item" {
                let text = &source[child.byte_range()];
                last_attr_is_repr_c = text.contains("repr") && text.contains("C");
            } else if child.kind() != "line_comment" && child.kind() != "block_comment" {
                last_attr_is_repr_c = false;
            }
        }
    }

    // Also check child attributes
    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "attribute" {
            let text = &source[child.byte_range()];
            if text.contains("repr") && text.contains("C") {
                return true;
            }
        }
    }

    false
}

/// Check all fields of a repr(C) struct for ABI-unsafe types.
fn check_fields(struct_node: Node, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let struct_name = struct_node
        .child_by_field_name("name")
        .map(|n| &ctx.source[n.byte_range()])
        .unwrap_or("<unknown>");

    let mut cursor = struct_node.walk();
    for child in struct_node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            check_field_list(child, struct_name, ctx, errors);
        }
    }
}

fn check_field_list(node: Node, struct_name: &str, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration" {
            check_single_field(child, struct_name, ctx, errors);
        }
    }
}

fn check_single_field(field: Node, struct_name: &str, ctx: &LintContext, errors: &mut Vec<LintError>) {
    let field_text = &ctx.source[field.byte_range()];
    let line = field.start_position().row + 1;

    // Check for ABI-unsafe types in the field's type annotation
    let type_part = match field_text.split(':').nth(1) {
        Some(t) => t.trim(),
        None => return,
    };

    for &(unsafe_type, fix) in ABI_UNSAFE_TYPES {
        if type_contains(type_part, unsafe_type) {
            // No suppression allowed for repr(C) ABI safety.
            // lint:allow(bare_collection) does NOT override this.
            errors.push(LintError {
                crate_name: ctx.crate_name.to_string(),
                line,
                lint_name: LINT_NAME,
                severity: crate::Severity::HARD_ERROR,
                message: format!(
                    "`{unsafe_type}` in `#[repr(C)]` struct `{struct_name}` is not ABI-safe — {fix}",
                ),
                finding_kind: None,
            });
            // one error per field (the first unsafe type found)
            return;
        }
    }
}

/// Check if a type string contains a specific type name as a standalone
/// identifier (not as part of a longer name).
fn type_contains(type_str: &str, type_name: &str) -> bool {
    // Match "TypeName<" or "TypeName," or "TypeName " or standalone "TypeName"
    for (i, _) in type_str.match_indices(type_name) {
        // Check that the character before (if any) is not alphanumeric
        if i > 0 {
            let prev = type_str.as_bytes()[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        // Check that the character after (if any) is not alphanumeric (except <)
        let after_idx = i + type_name.len();
        if after_idx < type_str.len() {
            let next = type_str.as_bytes()[after_idx];
            if next.is_ascii_alphanumeric() || next == b'_' {
                continue;
            }
        }
        return true;
    }
    false
}
