//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the registry lives: the directory that stays the default, a directory mounted on a ref,
//! and the `[[registry.store]]` declaration. Real git and a real bare repository throughout.

use super::*;
use crate::registry::test_world::*;
use crate::registry::{RegistryWriter, config_unknown_keys, load_registry_from};

#[test]
fn the_directory_stays_the_default_and_reads_as_it_always_did() {
    let w = World::new();
    let cfg = w.clone_of("a", false);
    assert!(cfg.registry_stores.is_empty());
    let reg_dir = cfg.mock_dir.join("registry");
    for (rel, text) in [
        ("a.toml", row("one", "1")),
        ("a/x.toml", row("two", "2")),
        ("a/b/y.toml", row("four", "4")),
        ("a-b.toml", row("five", "5")),
        (".hidden/h.toml", row("no", "n")),
        ("note.txt", "x".into()),
    ] {
        std::fs::create_dir_all(reg_dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(reg_dir.join(rel), text).unwrap();
    }
    let reg = load_registry_from(
        &cfg.registry_stores().unwrap(),
        &cfg.registry_namespaces,
        None,
    )
    .unwrap();
    assert_eq!(ids(&reg), [
        "ticket::five",
        "ticket::four",
        "ticket::one",
        "ticket::two"
    ]);
    assert_eq!(
        reg.files_read,
        [
            reg_dir.join("a/b/y.toml"),
            reg_dir.join("a/x.toml"),
            reg_dir.join("a-b.toml"),
            reg_dir.join("a.toml")
        ],
        "a directory is met in place among its siblings, by name, and a dot directory not at all"
    );
    let applied = RegistryWriter::new(&cfg, &cfg.registry_stores().unwrap())
        .write(
            &put(
                "a.toml",
                &format!("{}{}", row("one", "1"), row("three", "3")),
            ),
            "m",
        )
        .unwrap();
    assert!(applied.commit.is_none(), "a directory keeps no history");
    assert!(
        std::fs::read_to_string(reg_dir.join("a.toml"))
            .unwrap()
            .contains("three")
    );
    assert_eq!(w.remote_ref(), None, "nothing went near a remote");
}

#[test]
fn a_mounted_directory_hides_what_the_tree_still_holds_under_its_name() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let old = cfg.mock_dir.join("registry/ticket/old.toml");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, row("stale", "S")).unwrap();
    std::fs::write(cfg.mock_dir.join("registry/other.toml"), row("kept", "K")).unwrap();
    let s = stores(&cfg);
    RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/new.toml", &row("fresh", "F")), "m")
        .unwrap();
    let reg = load_registry_from(&s, &cfg.registry_namespaces, None).unwrap();
    assert_eq!(ids(&reg), ["ticket::fresh", "ticket::kept"]);
    assert!(
        !reg.files_read.contains(&old),
        "the tree copy is not even listed as read"
    );
}

#[test]
fn one_write_may_not_span_two_stores() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let changes = [
        (
            "ticket/a.toml".to_string(),
            Some(row("a", "A").into_bytes()),
        ),
        ("other.toml".to_string(), Some(row("b", "B").into_bytes())),
    ];
    let err = RegistryWriter::new(&cfg, &s)
        .write(&changes, "m")
        .unwrap_err();
    assert!(matches!(err, RegistryStoreError::SpansStores(_)), "{err}");
    assert_eq!(w.remote_ref(), None);
}

#[test]
fn a_store_declaration_is_read_and_a_misspelt_key_in_it_is_reported() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    assert_eq!(cfg.registry_stores.len(), 1);
    assert_eq!(cfg.registry_stores[0].dir, "ticket");
    let text = format!("{CONFIG}{STORE_DECL}fom = \"ref\"\n");
    let findings = config_unknown_keys(&text);
    assert!(
        findings
            .iter()
            .any(|f| f.message.contains("`fom`") && f.message.contains("registry.store")),
        "{findings:?}"
    );
    assert!(
        config_unknown_keys(&format!("{CONFIG}{STORE_DECL}")).is_empty(),
        "control: the declaration alone is clean"
    );
}

#[test]
fn a_form_that_is_neither_branch_nor_ref_is_refused_at_open() {
    let w = World::new();
    let cfg = w.clone_of("a", false);
    let decl = RegistryStoreDecl {
        dir: "ticket".into(),
        form: Some("both".into()),
        ..Default::default()
    };
    let err = RegistryStores::from_decls(&cfg.mock_dir, &cfg.repo_root, &[decl])
        .err()
        .unwrap();
    assert!(matches!(err, RegistryStoreError::Declared(_)), "{err}");
}

