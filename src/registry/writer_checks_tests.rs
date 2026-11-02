//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The checks of the registry writer: what it refuses before anything reaches a store, and what
//! it lets through. Real git and a real bare repository throughout.

use super::*;
use crate::registry::test_world::*;

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

fn found(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_second_finding_of_the_same_words_is_new() {
    // the writer's findings are counted: a write that adds a second row failing the same way
    // as one the registry already had raises one more finding than before
    let err = super::refuse_new(
        &found(&["[duplicate-id] x"]),
        found(&["[duplicate-id] x", "[duplicate-id] x"]),
    )
    .unwrap_err();
    assert!(
        matches!(&err, RegistryStoreError::Refused(f) if f == &["[duplicate-id] x"]),
        "{err}"
    );
}

#[test]
fn findings_the_registry_already_had_are_not_new_however_many() {
    let had = found(&["[a] x", "[a] x", "[b] y"]);
    super::refuse_new(&had, had.clone()).expect("the same findings, counted the same");
    super::refuse_new(&had, found(&["[b] y", "[a] x"])).expect("fewer findings is not new");
}

/// A document of the tree resolved against the registry as `stores` holds it now, and the tokens
/// the generation's own scan (`unresolved_in_generated`) finds left in it.
fn dangling_in(cfg: &Config, stores: &RegistryStores, document: &str) -> Vec<String> {
    let reg = load_registry_from(stores, &cfg.registry_namespaces, None).unwrap();
    let rendered = resolve_all(
        document,
        &cfg.registry_namespaces,
        &reg,
        &cfg.registry_roots,
        &cfg.repo_root,
        &cfg.docs_dir,
        cfg,
    );
    unresolved_in_generated(&rendered)
}

/// The gate's document-reference check (`refs_unresolved`) reads the documents generation wrote,
/// and the writer reads rows. A document names a row from a template the generation plan
/// enumerates (the crate map is read from cargo, the placeholders from the whole run), and
/// `render_all` writes into the tree's documents directory, so the writer has neither the
/// documents to look at nor a place to render them without running the whole generation per
/// write. This states what that leaves: the writer accepts the removal of a row a document names,
/// and the document is dangling afterwards. A full generation run reports it, and so does a commit concluding a merge and the
/// post-merge and post-rewrite hooks; a plain lint-only commit or push does not.
#[test]
fn a_known_gap_the_writer_lets_a_row_a_document_names_be_removed() {
    let w = World::new();
    let cfg = w.clone_of("a", true);
    let s = stores(&cfg);
    let writer = RegistryWriter::new(&cfg, &s);
    writer
        .write(&put("ticket/t.toml", &row("one", "One")), "add one")
        .unwrap();
    let document = "Tracked as {{ ticket::one }}.\n";
    assert!(
        dangling_in(&cfg, &s, document).is_empty(),
        "control: with the row present the reference resolves"
    );
    writer
        .write(&[("ticket/t.toml".to_string(), None)], "remove one")
        .expect("no check of the writer reads a document");
    assert_eq!(
        dangling_in(&cfg, &s, document),
        ["{{ ticket::one }}"],
        "the document dangles now, and only the scan of a generation run sees it"
    );
}
