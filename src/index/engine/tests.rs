use std::path::Path;

use super::captures_dir;

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
