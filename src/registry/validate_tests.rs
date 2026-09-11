//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The hand-kept lists in `validate.rs`, held to what they mirror.
//!
//! Neither a struct's serde keys nor the set of kinds a function can emit is
//! reachable from a test by reflection, so both are read out of the source.
//! The readers are narrow on purpose, and each has a planted case showing it
//! reports the drift it exists for, since a reader returning the empty set
//! agrees with nothing and one returning the list agrees with everything.

use std::collections::BTreeSet;

use super::{FIELD_KEYS, FINDING_KINDS, NAMESPACE_KEYS};

const MODEL: &str = include_str!("model.rs");

/// Every file that emits a registry finding kind, as source.
const PRODUCERS: [&str; 4] = [
    include_str!("validate.rs"),
    include_str!("resolve.rs"),
    include_str!("rowref/mod.rs"),
    include_str!("../entry/dispatch.rs"),
];

fn listed(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| (*k).to_string()).collect()
}

/// The keys serde reads for `pub struct <name>`: each `pub` field, under its
/// `rename` where an attribute gives one, with a raw identifier's `r#` dropped.
fn serde_keys(src: &str, name: &str) -> BTreeSet<String> {
    let open = format!("pub struct {name} {{");
    let (_, body) = src
        .split_once(open.as_str())
        .unwrap_or_else(|| panic!("no `{open}` in the source"));
    let (body, _) = body
        .split_once("\n}")
        .unwrap_or_else(|| panic!("`{name}` never closes"));
    let mut rename: Option<&str> = None;
    let mut keys = BTreeSet::new();
    for line in body.lines().map(str::trim) {
        if line.starts_with("#[serde(") {
            if let Some((_, rest)) = line.split_once("rename = \"") {
                rename = rest.split('"').next();
            }
        } else if let Some(rest) = line.strip_prefix("pub ") {
            let field = rest.split(':').next().unwrap_or("").trim();
            let field = field.strip_prefix("r#").unwrap_or(field);
            keys.insert(rename.take().unwrap_or(field).to_string());
        }
    }
    keys
}

/// Whether `kind` is emitted in `sources`: as a `kind:` field, as a match arm
/// handing one to it, or printed by the caller as `ERROR [<kind>]`. The list
/// carries each kind as a bare `"<kind>",` line, which is none of the three, so
/// listing a kind is not emitting it.
fn emitted(kind: &str, sources: &[&str]) -> bool {
    let quoted = format!("\"{kind}\"");
    let printed = format!("ERROR [{kind}]");
    sources.iter().any(|src| {
        src.contains(&printed)
            || src
                .lines()
                .map(str::trim)
                .any(|l| (l.starts_with("kind:") || l.contains("=> \"")) && l.contains(&quoted))
    })
}

/// Every kind a `kind:` field names in `sources`.
///
/// Fields only. A match arm or a printed `ERROR [..]` is not read here, because
/// both also carry strings that are not registry kinds, so a kind emitted only
/// that way and missing from the list is not caught by this reader.
fn kinds_named(sources: &[&str]) -> BTreeSet<String> {
    sources
        .iter()
        .flat_map(|s| s.lines())
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("kind:"))
        .filter_map(|r| r.trim().strip_prefix('"')?.split('"').next())
        .map(str::to_string)
        .collect()
}

#[test]
fn namespace_keys_match_the_struct() {
    assert_eq!(
        listed(NAMESPACE_KEYS),
        serde_keys(MODEL, "RegistryNamespace"),
        "a key the struct reads and the list lacks is reported as unknown in a \
         valid config, and one the list has and the struct lacks is accepted and dropped"
    );
}

#[test]
fn field_keys_match_the_struct() {
    assert_eq!(
        listed(FIELD_KEYS),
        serde_keys(MODEL, "RegistryField"),
        "the field-level list drifts the same way the namespace-level one can"
    );
}

#[test]
fn the_key_reader_takes_a_rename_and_a_raw_identifier() {
    let src = "pub struct Planted {\n    /// A doc line that says pub nothing: u8.\n    \
               #[serde(rename = \"field\")]\n    #[serde(default)]\n    pub fields: Vec<u8>,\n    \
               pub r#type: String,\n    pub plain: u8,\n}\npub struct After {\n    pub not_this: u8,\n}\n";
    assert_eq!(
        serde_keys(src, "Planted"),
        listed(&["field", "type", "plain"])
    );
}

#[test]
fn finding_kinds_are_producible() {
    let unproduced: Vec<&str> = FINDING_KINDS
        .iter()
        .copied()
        .filter(|k| !emitted(k, &PRODUCERS))
        .collect();
    assert!(
        unproduced.is_empty(),
        "listed and never emitted, so a severity configured for one governs nothing: {unproduced:?}"
    );
}

#[test]
fn every_named_kind_is_listed() {
    let missing: Vec<String> = kinds_named(&PRODUCERS)
        .into_iter()
        .filter(|k| !FINDING_KINDS.contains(&k.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "emitted and not listed, which is how three kinds once went missing from it: {missing:?}"
    );
}

#[test]
fn the_kind_reader_tells_listing_from_emitting() {
    let src = "const KINDS: &[&str] = &[\n    \"listed-only\",\n];\nRegistryFinding {\n    \
               kind: \"by-field\",\n};\nlet k = match a {\n    A::B => \"by-arm\",\n};\n\
               eprintln!(\"  ERROR [by-print]: x\");\n";
    for k in ["by-field", "by-arm", "by-print"] {
        assert!(emitted(k, &[src]), "`{k}` is emitted in the planted source");
    }
    assert!(!emitted("listed-only", &[src]), "listing is not emitting");
    assert!(!emitted("absent", &[src]));
    assert_eq!(kinds_named(&[src]), listed(&["by-field"]));
}
