use std::path::PathBuf;

use super::*;
use crate::bootstrap::{activate, deactivate, is_active};

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(super) fn repo(tag: &str) -> PathBuf {
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
pub(super) fn have_shook() -> bool {
    Command::new("git")
        .args(["shook", "--help"])
        .output()
        .map(|o| o.status.success() || !o.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn only_gitshooks_own_directory_is_recognised() {
    let dir = repo("path");
    let ours = shook_dir(&dir).join("hooks");
    let ours = ours.to_string_lossy().to_string();
    assert!(is_shook_path(&dir, &ours));
    assert!(is_shook_path(&dir, &format!("{ours}/")));
    // spellings that end the same way, or name another clone's, are not ours
    let other = repo("path-other");
    let theirs = shook_dir(&other).join("hooks").to_string_lossy().to_string();
    for not in [
        ".husky".to_string(),
        "mock/target/hooks".to_string(),
        "shook/hooks".to_string(),
        "tools/shook/hooks".to_string(),
        "/r/notshook/hooks".to_string(),
        format!("{}/entries", shook_dir(&dir).display()),
        theirs,
    ] {
        assert!(!is_shook_path(&dir, &not), "{not}");
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&other);
}

#[test]
fn deactivating_while_shook_holds_the_key_never_touches_the_key() {
    let dir = repo("held");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("hooks")).unwrap();
    let key = sd.join("hooks").to_string_lossy().to_string();
    git(&dir, &["config", "--local", "core.hooksPath", &key]);
    // the gate has no manifest here, and an earlier activation saved another
    // tool's path: the ordinary restore would write it over git-shook's key
    git(&dir, &["config", "--local", "mockspace.previousHooksPath", ".husky"]);

    deactivate(&dir).unwrap();
    deactivate(&dir).unwrap();

    assert_eq!(
        git(&dir, &["config", "--local", "core.hooksPath"]),
        key,
        "twice over, with a saved path, and the key is still git-shook's"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_registration_gives_the_manifest_back_as_it_was() {
    let dir = repo("refused");
    let bin = dir.join("fakebin");
    fs::create_dir_all(&bin).unwrap();
    let shook = bin.join("git-shook");
    crate::test_exec::write_executable(&shook, "#!/bin/sh\necho no >&2\nexit 1\n");
    let hooks = dir.join("durable");
    let path = manifest_path(&dir);

    // none before: none after
    assert!(register_with(&dir, &hooks, Some(&bin)).is_err());
    assert!(!path.exists(), "a refusal left a manifest where there was none");

    // one before: the same text after
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "name = \"00-mockspace\"\n# old\n").unwrap();
    assert!(register_with(&dir, &hooks, Some(&bin)).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "name = \"00-mockspace\"\n# old\n");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_manifest_names_every_durable_hook_and_quotes_the_path() {
    let text = manifest_text(Path::new("/h/a \"b\"/hooks-v3"));
    assert!(text.starts_with("name = \"00-mockspace\"\n"), "{text}");
    for event in events() {
        assert!(
            text.contains(&format!(
                "[hooks.{event}]\nrun = [\"/h/a \\\"b\\\"/hooks-v3/{event}\"]"
            )),
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
    assert!(is_shook_path(&dir, &before), "{before}");

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
