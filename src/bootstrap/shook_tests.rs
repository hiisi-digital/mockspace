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
fn a_hook_chains_to_the_repositorys_own_unless_it_runs_as_a_shook_entry() {
    // the control: the same generated text chains without the variable
    let generated = crate::bootstrap::chain_user_hook(Path::new("/r/.git/hooks/pre-commit"), false);
    let guard = generated.find(UNDER_SHOOK).expect("the generated chain reads the variable");
    let executable_check = generated.find("[ -x \"$USER_HOOK\" ]").expect("and still chains");
    assert!(guard < executable_check, "the variable is read before anything chains");
    let durable = mockspace_manifest::gate::chain_tail("pre-commit", "");
    assert!(durable.contains(UNDER_SHOOK), "the durable hook reads it too");
}

#[test]
fn the_manifest_names_every_durable_hook_and_quotes_the_path() {
    let text = manifest_text(Path::new("/h/a \"b\"/hooks-v3"));
    assert!(text.starts_with("name = \"mockspace\"\n"), "{text}");
    for event in events() {
        assert!(
            text.contains(&format!(
                "[hooks.{event}]\nrun = [\"env\", \"MOCKSPACE_UNDER_SHOOK=1\", \"/h/a \\\"b\\\"/hooks-v3/{event}\"]"
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

#[test]
fn adopting_shook_in_a_gated_repo_runs_the_gate_once() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("once");
    let log = dir.join("gate.log");
    let hooks = dir.join("durable");
    fs::create_dir_all(&hooks).unwrap();
    for event in events() {
        let p = hooks.join(event);
        fs::write(
            &p,
            format!("#!/bin/sh\n{MANAGED_MARKER}\necho {event} >> {}\n", log.display()),
        )
        .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // the repository is gated the old way, by core.hooksPath on the durable dir
    git(&dir, &["config", "--local", "core.hooksPath", hooks.to_str().unwrap()]);
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    assert!(
        shook_dir(&dir).join("entries/pre-commit/00-existing").is_file(),
        "control: git-shook did keep the gate as an existing hook"
    );

    register(&dir, &hooks).unwrap();

    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "pre-commit").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_kept_hook_that_is_not_ours_survives_registration() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("kept");
    let hooks = dir.join("durable");
    fs::create_dir_all(&hooks).unwrap();
    for event in events() {
        let p = hooks.join(event);
        fs::write(&p, "#!/bin/sh\ntrue\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let own = dir.join(".git/hooks/pre-commit");
    fs::write(&own, "#!/bin/sh\necho foreign\n").unwrap();
    fs::set_permissions(&own, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    let kept = shook_dir(&dir).join("entries/pre-commit/00-existing");
    assert!(kept.is_file(), "control: the repository's own hook was kept");
    register(&dir, &hooks).unwrap();
    assert!(kept.is_file(), "a kept hook without the managed marker is not dropped");
    assert!(shook_dir(&dir).join("kept/pre-commit").is_file());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deactivating_after_shook_gave_up_the_key_takes_the_gate_off_it() {
    let dir = repo("stale");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("manifests")).unwrap();
    fs::write(manifest_path(&dir), "name = \"mockspace\"\n").unwrap();
    git(&dir, &["config", "--local", "core.hooksPath", "mock/target/hooks"]);
    assert!(is_active(&dir), "control: the gate holds the key");

    deactivate(&dir).unwrap();

    assert!(!has_manifest(&dir), "the stale registration is gone");
    assert_eq!(git(&dir, &["config", "--local", "core.hooksPath"]), "", "and so is the key");
    assert!(!is_active(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_manifest_alone_is_not_an_active_gate() {
    let dir = repo("alone");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("manifests")).unwrap();
    fs::write(manifest_path(&dir), "name = \"mockspace\"\n").unwrap();
    let hooks_path = sd.join("hooks").to_string_lossy().to_string();
    assert!(!is_live(&dir, Some(&hooks_path)), "no entries were registered");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_repository_without_shook_is_not_reported_active_by_the_shook_path() {
    let dir = repo("plain");
    assert!(!is_active(&dir));
    assert!(!has_manifest(&dir));
    let _ = fs::remove_dir_all(&dir);
}
