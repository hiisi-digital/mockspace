//! Shared utility for extracting backtick-wrapped Rust type/trait/macro names
//! from markdown text.
//!
//! Used by the deprecation comparison lint and potentially other lints that
//! need to find type references in documentation.

use std::collections::BTreeSet;

/// Extract all backtick-wrapped identifiers from markdown text.
///
/// Returns unique names like "StorageRecord", "Behavior", "define_marker!" etc.
/// Filters to likely Rust type/trait/macro names: starts with uppercase letter,
/// or ends with `!` (macro). Deduplicates and sorts for deterministic output.
pub fn extract_backtick_names(text: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    let mut chars = text.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch != '`' {
            continue;
        }

        // Skip code blocks (``` or more)
        if text[i..].starts_with("```") {
            // Skip past the code block
            // Consume all leading backticks
            while let Some(&(_, '`')) = chars.peek() {
                chars.next();
            }
            // Skip until closing ``` (or end of text)
            let mut consecutive_backticks = 0;
            for (_, c) in chars.by_ref() {
                if c == '`' {
                    consecutive_backticks += 1;
                    if consecutive_backticks >= 3 {
                        break;
                    }
                } else {
                    consecutive_backticks = 0;
                }
            }
            continue;
        }

        // Single backtick — find the closing backtick
        let start = i + 1;
        let mut end = None;
        for (j, c) in chars.by_ref() {
            if c == '`' {
                end = Some(j);
                break;
            }
            // Single backtick spans cannot cross newlines
            if c == '\n' {
                break;
            }
        }

        if let Some(end_pos) = end {
            let content = &text[start..end_pos];
            if is_rust_type_or_macro(content) {
                names.insert(content.to_string());
            }
        }
    }

    names.into_iter().collect()
}

/// Check if a backtick-wrapped string looks like a Rust type, trait, or macro name.
///
/// Matches:
/// - Names starting with uppercase letter (types/traits): `StorageRecord`, `Behavior`
/// - Names ending with `!` (macros): `define_marker!`, `vec!`
fn is_rust_type_or_macro(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Macro names end with `!`
    if name.ends_with('!') {
        let base = &name[..name.len() - 1];
        // Must have at least one char before the `!` and be a valid identifier
        return !base.is_empty() && base.chars().all(|c| c.is_alphanumeric() || c == '_');
    }

    // Type/trait names start with uppercase
    let first = name.chars().next().unwrap();
    if !first.is_uppercase() {
        return false;
    }

    // Rest should be valid identifier characters (allow :: for paths like Foo::Bar)
    name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_type_names() {
        let text = "Uses `StorageRecord` and `Behavior` for things.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Behavior", "StorageRecord"]);
    }

    #[test]
    fn extracts_macro_names() {
        let text = "Use `define_marker!` and `define_signal!` macros.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["define_marker!", "define_signal!"]);
    }

    #[test]
    fn mixed_types_and_macros() {
        let text = "`StorageRecord` uses `define_record!` for `MyType`.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["MyType", "StorageRecord", "define_record!"]);
    }

    #[test]
    fn skips_lowercase_identifiers() {
        let text = "The `field_name` and `some_var` are local.";
        let names = extract_backtick_names(text);
        assert!(names.is_empty());
    }

    #[test]
    fn skips_code_blocks() {
        let text = "```rust\nlet x = StorageRecord::new();\n```\nUses `Behavior`.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Behavior"]);
    }

    #[test]
    fn deduplicates() {
        let text = "`Foo` and `Foo` and `Foo` again.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Foo"]);
    }

    #[test]
    fn sorted_output() {
        let text = "`Zebra` then `Alpha` then `Middle`.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Alpha", "Middle", "Zebra"]);
    }

    #[test]
    fn empty_backticks() {
        let text = "Nothing in `` here.";
        let names = extract_backtick_names(text);
        assert!(names.is_empty());
    }

    #[test]
    fn path_types() {
        let text = "Uses `Foo::Bar` type.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Foo::Bar"]);
    }

    #[test]
    fn rejects_non_identifier_content() {
        let text = "`foo bar` and `123` and `!bang`.";
        let names = extract_backtick_names(text);
        assert!(names.is_empty());
    }

    #[test]
    fn handles_unclosed_backtick() {
        // In Markdown, backticks pair greedily: `StorageRecord and ` is one span,
        // then `Behavior` has no closing backtick. Neither produces a type name.
        let text = "Open `StorageRecord and `Behavior`.";
        let names = extract_backtick_names(text);
        assert!(names.is_empty());
    }

    #[test]
    fn handles_newline_in_backtick() {
        // Newline breaks the backtick span. The remaining backticks re-pair
        // greedily: `Record` pairs with ` and `, leaving `Behavior` unpaired.
        let text = "Open `Storage\nRecord` and `Behavior`.";
        let names = extract_backtick_names(text);
        assert!(names.is_empty());
    }

    #[test]
    fn properly_paired_backticks_after_break() {
        // After a newline-broken span, properly paired backticks still work.
        let text = "Open `Storage\nnewline. `Behavior` is here.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["Behavior"]);
    }

    #[test]
    fn macro_with_invalid_name_rejected() {
        let text = "`not a macro!` and `valid_macro!`.";
        let names = extract_backtick_names(text);
        assert_eq!(names, vec!["valid_macro!"]);
    }
}
