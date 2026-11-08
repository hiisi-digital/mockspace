use std::path::Path;

use super::{captures_dir, row_sources};
use crate::registry::{Registry, RegistryRow};

#[test]
fn a_row_s_source_is_shown_from_the_repository_root() {
    let root = Path::new("/work/repo");
    let mut reg = Registry::default();
    for (slug, file) in [
        ("a", "/work/repo/mock/registry/tenet/workflow.toml"),
        ("b", "/elsewhere/outside.toml"),
    ] {
        let row = RegistryRow {
            slug:      slug.into(),
            namespace: "tenet".into(),
            source:    file.into(),
            fields:    Default::default(),
        };
        reg.rows.insert(row.qualified(), row);
    }
    let canon = ["mock/registry/tenet/*.toml".to_string(), "/elsewhere/*.toml".to_string()];
    let got = row_sources(&reg, root, &canon);
    assert_eq!(got["tenet::a"], "mock/registry/tenet/workflow.toml");
    // THE CONTROL: a source outside the root is shown whole, not cut to something that looks inside.
    assert_eq!(got["tenet::b"], "/elsewhere/outside.toml");
}

#[test]
fn only_a_file_the_repository_declares_canon_gives_its_rows_a_source() {
    let root = Path::new("/work/repo");
    let mut reg = Registry::default();
    for (slug, ns, file) in [
        ("a", "tenet", "/work/repo/mock/registry/tenet/workflow.toml"),
        ("t", "ticket", "/work/repo/mock/registry/ticket/items/t.toml"),
    ] {
        let row = RegistryRow {
            slug:      slug.into(),
            namespace: ns.into(),
            source:    file.into(),
            fields:    Default::default(),
        };
        reg.rows.insert(row.qualified(), row);
    }
    let canon = ["mock/registry/tenet/*.toml".to_string()];
    let got = row_sources(&reg, root, &canon);
    assert!(got.contains_key("tenet::a"));
    // THE CONTROL: a ticket changes daily and says nothing settled, so it has no canon source.
    assert!(!got.contains_key("ticket::t"));
    assert!(row_sources(&reg, root, &[]).is_empty(), "no declared canon, no canon sources");
}

fn mk(root: &Path, rel: &str) {
    std::fs::create_dir_all(root.join(rel)).unwrap();
}

#[test]
fn captures_are_found_in_the_repository_or_in_any_directory_above_it() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path().join("work/repo");
    mk(t.path(), "work/repo");
    let declared = [".data/op-responses".to_string()];
    assert_eq!(captures_dir(&repo, &declared, None), None, "nothing declared exists yet");

    mk(t.path(), ".data/op-responses");
    assert_eq!(
        captures_dir(&repo, &declared, None),
        Some(t.path().join(".data/op-responses")),
        "the workspace around the repository"
    );
}

#[test]
fn a_sibling_clone_is_a_declared_path_like_any_other() {
    let t = tempfile::tempdir().unwrap();
    mk(t.path(), "kaski");
    mk(t.path(), "clause-dev/.data/op-responses");
    let declared = [".data/op-responses".to_string(), "clause-dev/.data/op-responses".to_string()];
    assert_eq!(
        captures_dir(&t.path().join("kaski"), &declared, None),
        Some(t.path().join("clause-dev/.data/op-responses"))
    );
}

#[test]
fn a_named_directory_wins_and_a_mistyped_one_finds_nothing_rather_than_something_else() {
    let t = tempfile::tempdir().unwrap();
    mk(t.path(), ".data/op-responses");
    mk(t.path(), "elsewhere");
    let declared = [".data/op-responses".to_string()];
    let elsewhere = t.path().join("elsewhere");
    assert_eq!(
        captures_dir(t.path(), &declared, elsewhere.to_str()),
        Some(elsewhere.clone())
    );
    // THE CONTROL: the declared path exists, and the mistyped name still does not fall back to it.
    assert_eq!(captures_dir(t.path(), &declared, Some("/no/such/dir")), None);
    assert_eq!(captures_dir(t.path(), &[], None), None, "no declaration, no captures");
}
