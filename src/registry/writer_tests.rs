//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry on a ref: the race, the checks of the writer, a fetch that fails, and the
//! directory that stays the default. Real git and a real bare repository throughout.
use std::path::PathBuf;

use mockspace_store::{RefPair, RefStore, RetryPolicy, StoreError};

use super::*;
use crate::config::Config;
use crate::test_remote::{Remote, git};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work to do."

[[registry.namespace.field]]
name = "title"
type = "string"
required = true
description = "What."

[[registry.namespace.field]]
name = "answers"
type = "string[]"
description = "What was said."
"#;

const STORE_DECL: &str = "\n[[registry.store]]\ndir = \"ticket\"\n";

/// A bare remote and any number of clones of it, each a project with the config above.
struct World {
    remote: Remote,
    bare:   PathBuf,
    tmp:    PathBuf,
}

const TICKET_REF: &str = "refs/heads/mock/registry/ticket";

impl World {
    fn new() -> Self {
        let remote = Remote::new();
        let (bare, tmp) = (remote.bare.clone(), remote.tmp.path().to_path_buf());
        Self {
            remote,
            bare,
            tmp,
        }
    }

    /// A clone whose project mounts `ticket` on the ref (`mounted`) or keeps it in the tree.
    fn clone_of(&self, name: &str, mounted: bool) -> Config {
        let root = self.remote.clone_dir(name);
        let config = format!("{CONFIG}{}", if mounted { STORE_DECL } else { "" });
        std::fs::write(root.join("mock/mockspace.toml"), config).unwrap();
        let cfg = Config::from_dir(&root.join("mock"));
        crate::registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
        cfg
    }

    fn remote_ref(&self) -> Option<String> {
        self.remote.tip(TICKET_REF)
    }

    fn remote_file(&self, path: &str) -> Option<String> {
        self.remote.file(TICKET_REF, path)
    }
}
/// The ref-mounted stores of `cfg`, with no waiting between attempts.
fn stores(cfg: &Config) -> RegistryStores {
    let pair = RefPair::branch_form("registry/ticket").unwrap();
    let store = RefStore::new(&cfg.repo_root, "origin", pair).with_retry(RetryPolicy::immediate(4));
    RegistryStores::directory(&cfg.mock_dir)
        .mount("ticket", Box::new(store))
        .unwrap()
}

fn row(id: &str, title: &str) -> String {
    format!("[[ticket]]\nid = \"{id}\"\ntitle = \"{title}\"\n\n")
}

fn put(rel: &str, text: &str) -> Vec<(String, Option<Vec<u8>>)> {
    vec![(rel.to_string(), Some(text.as_bytes().to_vec()))]
}

fn ids(reg: &Registry) -> Vec<String> {
    reg.rows.keys().cloned().collect()
}

#[test]
fn a_write_lands_on_the_ref_and_reads_back_through_the_loader() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let applied = RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", &row("one", "One")), "add one")
        .unwrap();
    assert!(applied.changed && applied.commit.is_some());
    assert!(w.remote_file("t.toml").unwrap().contains("id = \"one\""));
    let reg = load_registry_from(&s, &cfg.registry_namespaces, None).unwrap();
    assert_eq!(ids(&reg), ["ticket::one"]);
    assert_eq!(
        reg.rows["ticket::one"].source,
        cfg.mock_dir.join("registry/ticket/t.toml"),
        "a row says it was declared where it would sit in the tree, whichever store holds it"
    );
}

#[test]
fn a_lost_race_is_resolved_by_the_registry_merge_for_rows_added_on_both_sides() {
    let w = World::new();
    let (a, b) = (w.clone_of("a", true), w.clone_of("b", true));
    let (sa, sb) = (stores(&a), stores(&b));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("first", "First")), "seed")
        .unwrap();
    // B reads the registry as it is, then A writes, then B writes on what it read.
    let seen = load_registry_from(&sb, &b.registry_namespaces, None).unwrap();
    assert_eq!(ids(&seen), ["ticket::first"]);
    let ours_a = format!("{}{}", row("first", "First"), row("from_a", "A"));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &ours_a), "a adds")
        .unwrap();
    let ours_b = format!("{}{}", row("first", "First"), row("from_b", "B"));
    RegistryWriter::new(&b, &sb)
        .write(&put("ticket/t.toml", &ours_b), "b adds")
        .unwrap();
    let text = w.remote_file("t.toml").unwrap();
    for id in ["first", "from_a", "from_b"] {
        assert!(
            text.contains(&format!("id = \"{id}\"")),
            "{id} is missing from:\n{text}"
        );
    }
}

#[test]
fn a_row_changed_two_ways_is_not_merged_and_the_winner_stands() {
    let w = World::new();
    let (a, b) = (w.clone_of("a", true), w.clone_of("b", true));
    let (sa, sb) = (stores(&a), stores(&b));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("x", "Base")), "seed")
        .unwrap();
    load_registry_from(&sb, &b.registry_namespaces, None).unwrap();
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("x", "From A")), "a")
        .unwrap();
    let tip = w.remote_ref();
    let err = RegistryWriter::new(&b, &sb)
        .write(&put("ticket/t.toml", &row("x", "From B")), "b")
        .unwrap_err();
    assert!(
        matches!(err, RegistryStoreError::Store {
            source: StoreError::Merge { .. },
            ..
        }),
        "{err}"
    );
    assert_eq!(
        w.remote_ref(),
        tip,
        "the refused write left the remote where the winner put it"
    );
}

#[test]
fn a_write_the_checks_refuse_never_reaches_the_ref() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let writer = RegistryWriter::new(&cfg, &s);
    writer
        .write(&put("ticket/a.toml", &row("same", "A")), "seed")
        .unwrap();
    let tip = w.remote_ref();
    for (what, rel, text) in [
        (
            "a second declaration of an id",
            "ticket/b.toml",
            row("same", "B"),
        ),
        (
            "text that is not TOML",
            "ticket/c.toml",
            "[[ticket\nid = ".to_string(),
        ),
        (
            "a row with no id",
            "ticket/d.toml",
            "[[ticket]]\ntitle = \"no id\"\n".to_string(),
        ),
    ] {
        let err = writer.write(&put(rel, &text), "bad").unwrap_err();
        assert!(
            matches!(err, RegistryStoreError::Refused(_)),
            "{what}: {err}"
        );
        assert_eq!(w.remote_ref(), tip, "{what} reached the ref");
    }
}

#[test]
fn a_write_the_checks_pass_alone_but_refuse_after_the_rebase_never_lands() {
    let w = World::new();
    let (a, b) = (w.clone_of("a", true), w.clone_of("b", true));
    let (sa, sb) = (stores(&a), stores(&b));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/seed.toml", &row("seed", "S")), "seed")
        .unwrap();
    load_registry_from(&sb, &b.registry_namespaces, None).unwrap();
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/a.toml", &row("dup", "A")), "a")
        .unwrap();
    let tip = w.remote_ref();
    let err = RegistryWriter::new(&b, &sb)
        .write(&put("ticket/b.toml", &row("dup", "B")), "b")
        .unwrap_err();
    assert!(err.to_string().contains("duplicate-id"), "{err}");
    assert_eq!(
        w.remote_ref(),
        tip,
        "the rebased write was checked on the tip that won"
    );
}

#[test]
fn a_failed_fetch_is_an_error_and_never_an_empty_registry() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    git(&cfg.repo_root, &[
        "remote",
        "set-url",
        "origin",
        w.tmp.join("gone.git").to_str().unwrap(),
    ]);
    let s = stores(&cfg);
    let err = load_registry_from(&s, &cfg.registry_namespaces, None).unwrap_err();
    assert!(
        matches!(err, RegistryStoreError::Store {
            source: StoreError::Fetch { .. },
            ..
        }),
        "{err}"
    );
    let err = RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", &row("x", "X")), "m")
        .unwrap_err();
    assert!(matches!(err, RegistryStoreError::Store { .. }), "{err}");
    // the control: a remote that answers, with nothing on the ref, is an empty registry
    let w2 = World::new();
    let ok = w2.clone_of("ok", true);
    assert_eq!(
        load_registry_from(&stores(&ok), &ok.registry_namespaces, None)
            .unwrap()
            .rows
            .len(),
        0
    );
}

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

#[test]
fn a_row_the_schema_refuses_never_reaches_the_ref() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let w_ = RegistryWriter::new(&cfg, &s);
    // a row with an id and no `title`, which the fixture declares required
    let bad = "[[ticket]]\nid = \"untitled\"\n\n";
    let err = w_
        .write(&put("ticket/t.toml", bad), "add untitled")
        .unwrap_err();
    assert!(
        matches!(&err, RegistryStoreError::Refused(f) if f.iter().any(|p| p.starts_with("[schema]"))),
        "{err}"
    );
    assert_eq!(
        w.remote_ref(),
        None,
        "a refused write leaves the ref unborn"
    );
    // the control: the same row with its title lands
    w_.write(
        &put("ticket/t.toml", &row("untitled", "Now titled")),
        "add it",
    )
    .unwrap();
    assert!(w.remote_file("t.toml").unwrap().contains("Now titled"));
}

#[test]
fn a_failed_fetch_reaches_the_callers_as_an_error_too() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    git(&cfg.repo_root, &[
        "remote",
        "set-url",
        "origin",
        w.tmp.join("gone.git").to_str().unwrap(),
    ]);
    assert!(
        matches!(
            load_registry_for(&cfg),
            Err(RegistryStoreError::Store {
                source: StoreError::Fetch { .. },
                ..
            })
        ),
        "the engine's load"
    );
    let stores = cfg.registry_stores().unwrap();
    assert!(
        load_registry_for_expr_from(&stores, &cfg.registry_namespaces, "ticket").is_err(),
        "the query's load"
    );
    // the control: the same project with a reachable remote loads
    let fine = World::new();
    let ok = fine.clone_of("b", true);
    assert!(load_registry_for(&ok).is_ok());
}

#[test]
fn the_checks_that_need_no_taplo_refuse_on_their_own() {
    let w = World::new();
    let cfg = w.clone_of("a", false);
    let s = RegistryStores::directory(&cfg.mock_dir);
    let writer = RegistryWriter::new(&cfg, &s);
    let changed: BTreeSet<String> = ["bad.toml".to_string(), "noid.toml".to_string()].into();
    let files: Files = vec![
        ("bad.toml".into(), Some("[[ticket]\nid =".into())),
        (
            "noid.toml".into(),
            Some("[[ticket]]\ntitle = \"x\"\n".into()),
        ),
    ];
    let found = writer.problems(&files, &changed, false);
    assert!(
        found.iter().any(|p| p.starts_with("[not-toml] bad.toml")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|p| p.starts_with("[row-without-id]")),
        "{found:?}"
    );
    assert!(
        writer
            .problems(
                &vec![("ok.toml".into(), Some(row("a", "A")))],
                &["ok.toml".to_string()].into(),
                false
            )
            .is_empty()
    );
    // and through the write, on a directory: the refused file is not written
    let err = writer
        .write(&put("bad.toml", "[[ticket]\nid ="), "x")
        .unwrap_err();
    assert!(matches!(err, RegistryStoreError::Refused(_)), "{err}");
    assert!(!cfg.mock_dir.join("registry/bad.toml").exists());
}

#[test]
fn a_schema_failure_already_in_the_tree_does_not_block_a_valid_ref_write() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let tree = cfg.mock_dir.join("registry/other.toml");
    std::fs::create_dir_all(tree.parent().unwrap()).unwrap();
    std::fs::write(&tree, "[[ticket]]\nid = \"untitled\"\n").unwrap();
    let s = stores(&cfg);
    RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", &row("one", "One")), "add one")
        .expect("a finding the registry already had is not this write's");
    assert!(w.remote_file("t.toml").is_some());
}

#[test]
fn a_reference_cycle_made_by_a_write_is_refused() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let cyc = "[[ticket]]\nid = \"a\"\ntitle = \"{{ ticket::b::title }}\"\n\n[[ticket]]\nid = \"b\"\ntitle = \"{{ ticket::a::title }}\"\n";
    let err = RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", cyc), "cycle")
        .unwrap_err();
    assert!(
        matches!(&err, RegistryStoreError::Refused(f) if f.iter().any(|p| p.contains("cycle"))),
        "{err}"
    );
    assert_eq!(w.remote_ref(), None);
}
