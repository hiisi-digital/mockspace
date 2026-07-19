#![allow(unused_imports)]
use super::*;

#[test]
fn adds_catch_all_target_to_empty_repo() {
    let dir = tempfile::tempdir().unwrap();
    let mut actions = Vec::new();
    ensure_gitignore(dir.path(), &mut actions);
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(
        content.lines().any(|l| l.trim() == "target/"),
        "expected a catch-all `target/` line, got:\n{content}"
    );
    assert_eq!(actions.len(), 1, "expected one action recorded");
}

#[test]
fn preserves_existing_entries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".gitignore"), "/target\n.DS_Store\n*.swp\n").unwrap();
    let mut actions = Vec::new();
    ensure_gitignore(dir.path(), &mut actions);
    let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(
        content.contains(".DS_Store"),
        "clobbered existing entries:\n{content}"
    );
    assert!(
        content.contains("*.swp"),
        "clobbered existing entries:\n{content}"
    );
    assert!(
        content.lines().any(|l| l.trim() == "target/"),
        "did not add catch-all target/:\n{content}"
    );
}

#[test]
fn idempotent_when_catch_all_present() {
    let dir = tempfile::tempdir().unwrap();
    let mut actions = Vec::new();
    ensure_gitignore(dir.path(), &mut actions);
    let after_first = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    let mut actions2 = Vec::new();
    ensure_gitignore(dir.path(), &mut actions2);
    let after_second = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(after_first, after_second, "second run mutated the file");
    assert!(
        actions2.is_empty(),
        "second run recorded an action: {actions2:?}"
    );
}
