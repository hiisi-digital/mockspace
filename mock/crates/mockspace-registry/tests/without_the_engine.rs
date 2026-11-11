//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A registry opened, read, checked and queried by a tool that has this crate and no engine.
//!
//! Only the public surface is in reach here, so what these tests do is what such a tool can do.

use std::path::Path;

use mockspace_registry::{Env, RegistryEnv, load_registry_for, validate, validate_row_references};

/// A config as an engine project writes it: keys of the engine's own around the registry's.
const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"
mockspace_branch = "dev"

[lints]
"no-bare-pub" = "error"

[[registry.namespace]]
key = "ticket"
title = "Tickets"

[[registry.namespace.field]]
name = "title"
type = "string"
required = true

[[registry.namespace.field]]
name = "needs"
type = "ticket[]"

[ref.roots.notes]
path = "notes"
frozen = true
"#;

fn project(config: &str, rows: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    std::fs::create_dir_all(mock.join("registry/ticket")).unwrap();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    std::fs::write(mock.join("mockspace.toml"), config).unwrap();
    std::fs::write(mock.join("registry/ticket/open.toml"), rows).unwrap();
    tmp
}

fn findings(env: &Env) -> Vec<&'static str> {
    let reg = load_registry_for(env).unwrap();
    validate(env.namespaces(), &reg)
        .into_iter()
        .chain(validate_row_references(&reg, env.namespaces()))
        .map(|f| f.kind)
        .collect()
}

#[test]
fn the_declaration_is_read_from_the_config_and_the_engines_keys_are_passed_over() {
    let tmp = project(CONFIG, "");
    let env = Env::from_dir(&tmp.path().join("mock")).unwrap();
    assert_eq!(env.repo_root, tmp.path());
    assert!(env.namespaces.iter().any(|n| n.key == "ticket"), "the declared namespace");
    assert!(env.namespaces.len() > 1, "the built-in namespaces come with it");
    assert_eq!(env.roots.get("notes").map(String::as_str), Some("notes"));
    assert!(env.frozen_roots.contains("notes"));
    // a config at the repository root is read where the mock directory has none
    std::fs::rename(tmp.path().join("mock/mockspace.toml"), tmp.path().join("mockspace.toml")).unwrap();
    let moved = Env::from_dir(&tmp.path().join("mock")).unwrap();
    assert!(moved.namespaces.iter().any(|n| n.key == "ticket"));
}

#[test]
fn a_config_that_does_not_parse_is_an_error_and_an_absent_one_declares_nothing() {
    let tmp = project("[[registry.namespace]\nkey = ", "");
    let why = Env::from_dir(&tmp.path().join("mock")).unwrap_err();
    assert!(why.contains("does not parse"), "{why}");
    std::fs::remove_file(tmp.path().join("mock/mockspace.toml")).unwrap();
    let none = Env::from_dir(&tmp.path().join("mock")).unwrap();
    assert!(none.namespaces.iter().all(|n| n.key != "ticket"));
}

#[test]
fn rows_are_loaded_and_checked_and_a_reference_to_no_row_is_found() {
    let good = "[[ticket]]\nid = \"one\"\ntitle = \"first\"\n\n[[ticket]]\nid = \"two\"\ntitle = \"second\"\nneeds = [\"one\"]\n";
    let tmp = project(CONFIG, good);
    let env = Env::from_dir(&tmp.path().join("mock")).unwrap();
    let reg = load_registry_for(&env).unwrap();
    assert_eq!(reg.rows.keys().collect::<Vec<_>>(), ["ticket::one", "ticket::two"]);
    assert_eq!(findings(&env), Vec::<&str>::new(), "the control: a registry in order has no finding");

    let dangling = good.replace("needs = [\"one\"]", "needs = [\"gone\"]");
    std::fs::write(tmp.path().join("mock/registry/ticket/open.toml"), dangling).unwrap();
    assert_eq!(findings(&env), ["unknown-row-reference"]);
}

#[test]
fn an_expression_is_answered_as_a_document_would_have_it() {
    let tmp = project(CONFIG, "[[ticket]]\nid = \"one\"\ntitle = \"first\"\n");
    let env = Env::from_dir(&tmp.path().join("mock")).unwrap();
    let reg = load_registry_for(&env).unwrap();
    let text = "see {{ticket::one::title}} and {{ticket::none::title}}";
    let out = mockspace_registry::resolve_all(
        text,
        env.namespaces(),
        &reg,
        env.roots(),
        env.repo_root(),
        Path::new("/nonexistent"),
        &env,
    );
    assert!(out.contains("see first"), "{out}");
    assert!(out.contains("ticket::none"), "a reference to no row is left to be reported: {out}");
}
