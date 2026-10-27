use std::path::PathBuf;

use super::*;
use crate::bootstrap::{activate, deactivate, is_active};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo(tag: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("ms-shook-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    dir
}

/// Whether `git shook` is on the path. The registering tests need the real
/// thing, and say so when they cannot run rather than passing.
fn have_shook() -> bool {
    Command::new("git")
        .args(["shook", "--help"])
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn only_gitshooks_own_directory_is_recognised() {
    assert!(is_shook_path("/r/.git/shook/hooks"));
    assert!(is_shook_path("/r/.git/shook/hooks/"));
    for other in [".husky", "mock/target/hooks", "/r/.git/shook/entries", "shook/hooks-old"] {
        assert!(!is_shook_path(other), "{other}");
    }
}

#[test]
fn the_manifest_names_every_durable_hook_and_quotes_the_path() {
    let text = manifest_text(Path::new("/h/a \"b\"/hooks-v3"));
    assert!(text.starts_with("name = \"mockspace\"\n"), "{text}");
    for event in events() {
        assert!(
            text.contains(&format!("[hooks.{event}]\nrun = [\"/h/a \\\"b\\\"/hooks-v3/{event}\"]")),
            "{event}: {text}"
        );
    }
}

#[test]
fn unregistering_removes_the_gate_and_leaves_other_entries() {
    let dir = repo("unreg");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("manifests")).unwrap();
    fs::write(manifest_path(&dir), "x").unwrap();
    for ev in ["pre-commit", "pre-push"] {
        fs::create_dir_all(sd.join("entries").join(ev)).unwrap();
        fs::write(sd.join("entries").join(ev).join(NAME), "").unwrap();
        fs::write(sd.join("entries").join(ev).join("homma"), "").unwrap();
    }
    assert!(has_manifest(&dir));
    unregister(&dir).unwrap();
    assert!(!has_manifest(&dir));
    assert!(!sd.join("entries/pre-commit").join(NAME).exists());
    assert!(sd.join("entries/pre-commit/homma").is_file(), "another tool's entry stays");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn activating_where_shook_holds_the_key_registers_and_leaves_the_key() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("act");
    let mock = dir.join("mock");
    fs::create_dir_all(mock.join("target").join("hooks")).unwrap();
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    let before = git(&dir, &["config", "--local", "core.hooksPath"]);
    assert!(is_shook_path(&before), "{before}");

    activate(&dir, &mock).unwrap();

    assert_eq!(git(&dir, &["config", "--local", "core.hooksPath"]), before, "the key is git-shook's still");
    assert!(has_manifest(&dir));
    assert!(is_active(&dir));
    let entry = shook_dir(&dir).join("entries/pre-commit");
    assert!(entry.join(NAME).is_file());
    assert!(
        entry.read_dir().unwrap().count() >= 2,
        "the gate sits beside the tracked tool's entry"
    );

    deactivate(&dir).unwrap();
    assert!(!is_active(&dir), "control: deactivated means not active");
    assert_eq!(git(&dir, &["config", "--local", "core.hooksPath"]), before);
    assert!(!entry.join(NAME).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_repository_without_shook_is_not_reported_active_by_the_shook_path() {
    let dir = repo("plain");
    assert!(!is_active(&dir));
    assert!(!has_manifest(&dir));
    let _ = fs::remove_dir_all(&dir);
}
