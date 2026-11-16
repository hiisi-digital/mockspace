//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital

//! Which staged files the rows gate takes as rows of a direct namespace, and which it hands
//! back to the full gate.

use super::{direct_namespaces, is_direct_row};

const CONFIG: &str = r#"
[[registry.namespace]]
key = "ticket"
console_direct = true

[[registry.namespace]]
key = "ticket_move"
console_direct = true

[[registry.namespace]]
key = "tenet"

[[registry.namespace]]
key = "note"
console_direct = false
"#;

fn direct() -> Vec<String> {
    direct_namespaces(CONFIG)
}

#[test]
fn only_namespaces_marked_console_direct_are_direct() {
    assert_eq!(direct(), vec![
        "ticket".to_string(),
        "ticket_move".to_string()
    ]);
    assert!(direct_namespaces("not = [toml").is_empty());
    assert!(direct_namespaces("").is_empty());
}

#[test]
fn a_ticket_row_with_its_moves_is_a_direct_row() {
    let text = "[[ticket]]\nid = \"a\"\n\n[[ticket_move]]\nticket = \"a\"\n";
    assert!(is_direct_row(
        "mock/registry/ticket/items/a.toml",
        text,
        "mock",
        &direct()
    ));
}

#[test]
fn anything_else_is_handed_back_to_the_full_gate() {
    let d = direct();
    let row = "[[ticket]]\nid = \"a\"\n";
    let table: &[(&str, &str, &str)] = &[
        (
            "a tenet beside the ticket",
            "mock/registry/ticket/items/a.toml",
            "[[ticket]]\nid = \"a\"\n[[tenet]]\nid = \"b\"\n",
        ),
        (
            "a namespace marked false",
            "mock/registry/note.toml",
            "[[note]]\nid = \"a\"\n",
        ),
        (
            "a plain key, not an array of tables",
            "mock/registry/ticket/kinds.toml",
            "ticket = 1\n",
        ),
        ("an empty file", "mock/registry/ticket/items/a.toml", ""),
        ("not toml", "mock/registry/ticket/items/a.toml", "[[ticket"),
        ("outside the registry", "mock/notes/a.toml", row),
        ("outside the mock directory", "registry/ticket/a.toml", row),
        ("not a toml file", "mock/registry/ticket/items/a.md", row),
    ];
    for (what, path, text) in table {
        assert!(
            !is_direct_row(path, text, "mock", &d),
            "{what}: taken as a direct row"
        );
    }
}

#[test]
fn the_mock_directory_is_the_declared_one() {
    let row = "[[ticket]]\nid = \"a\"\n";
    assert!(is_direct_row(
        "linux/registry/ticket/a.toml",
        row,
        "linux",
        &direct()
    ));
    assert!(!is_direct_row(
        "mock/registry/ticket/a.toml",
        row,
        "linux",
        &direct()
    ));
}
