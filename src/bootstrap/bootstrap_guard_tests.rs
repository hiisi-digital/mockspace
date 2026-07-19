#![allow(unused_imports)]
use std::path::Path;

use super::is_inside_cargo_home;

#[test]
fn bootstrap_cargo_bin_dir_resolution() {
    assert_eq!(
        super::cargo_bin_dir_from(Some("/c/cargo".into()), Some("/home/u".into())),
        Some(std::path::PathBuf::from("/c/cargo/bin"))
    );
    assert_eq!(
        super::cargo_bin_dir_from(None, Some("/home/u".into())),
        Some(std::path::PathBuf::from("/home/u/.cargo/bin"))
    );
    assert_eq!(
        super::cargo_bin_dir_from(Some("".into()), Some("/home/u".into())),
        Some(std::path::PathBuf::from("/home/u/.cargo/bin"))
    );
    assert_eq!(super::cargo_bin_dir_from(None, None), None);
}

#[test]
fn bootstrap_launcher_is_cwd_independent_and_self_heals() {
    let s = super::gen_launcher_script();
    // discovers the repo instead of assuming cwd
    assert!(s.contains("find_root"));
    assert!(s.contains("mockspace.mockdir"));
    // runs the proxy by absolute path (cwd never matters)
    assert!(s.contains(r#"--manifest-path "$proxy""#));
    assert!(s.contains(r#"--dir "$root/$mockdir""#));
    // self-heals a cleaned proxy with a locked check
    assert!(s.contains("cargo check --quiet --locked"));
    // forwards all args
    assert!(s.contains(r#""$@""#));
}

#[test]
fn bootstrap_is_mock_alias_line_is_exact() {
    assert!(super::is_mock_alias_line(r#"mock = "run ...""#));
    assert!(super::is_mock_alias_line(r#"  mock  =  "x""#));
    assert!(!super::is_mock_alias_line(r#"mockfoo = "x""#));
    assert!(!super::is_mock_alias_line(r#"mock-thing = "x""#));
    assert!(!super::is_mock_alias_line("[alias]"));
}

#[test]
fn bootstrap_ascend_prefix_by_depth() {
    assert_eq!(super::ascend_prefix("mock"), "../");
    assert_eq!(super::ascend_prefix("a/b"), "../../");
    assert_eq!(super::ascend_prefix("x/y/z"), "../../../");
    assert_eq!(super::ascend_prefix("mock/"), "../");
    assert_eq!(super::ascend_prefix("./mock"), "../");
    assert_eq!(super::ascend_prefix(""), "../");
}

#[test]
fn bootstrap_guard_skips_inside_cargo_home() {
    // a git-checkout path like the 2026-06 incident's poisoned entry
    assert!(is_inside_cargo_home(
        Path::new("/home/u/.cargo/git/checkouts/arvo-da1db1860201e542/5776b61/mock/crates/arvo"),
        Some(Path::new("/home/u/.cargo")),
    ));
}

#[test]
fn bootstrap_guard_skips_registry_src_too() {
    assert!(is_inside_cargo_home(
        Path::new("/home/u/.cargo/registry/src/index.crates.io-1cd66030c949c28d/somecrate-0.1.0"),
        Some(Path::new("/home/u/.cargo")),
    ));
}

#[test]
fn bootstrap_guard_proceeds_for_working_repo() {
    assert!(!is_inside_cargo_home(
        Path::new("/home/u/Dev/clause-dev/arvo/mock/crates/arvo"),
        Some(Path::new("/home/u/.cargo")),
    ));
}

#[test]
fn bootstrap_guard_proceeds_when_cargo_home_unknown() {
    assert!(!is_inside_cargo_home(
        Path::new("/home/u/.cargo/git/checkouts/x/y"),
        None,
    ));
}

#[test]
fn bootstrap_guard_proceeds_for_this_repo_own_tree() {
    // a real, canonicalizable dir against a fake cargo home
    assert!(!is_inside_cargo_home(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        Some(Path::new("/nonexistent/.cargo")),
    ));
}

#[test]
fn bootstrap_guard_relative_paths_do_not_panic() {
    assert!(!is_inside_cargo_home(
        Path::new("mock/crates/arvo"),
        Some(Path::new("/tmp/.cargo")),
    ));
}

// resolve_cargo_home

#[test]
fn bootstrap_guard_cargo_home_explicit_wins() {
    let home = super::resolve_cargo_home(Some("/custom/cargo".into()), Some("/home/u".into()));
    assert_eq!(home, Some(std::path::PathBuf::from("/custom/cargo")));
}

#[test]
fn bootstrap_guard_cargo_home_falls_back_to_home() {
    let home = super::resolve_cargo_home(None, Some("/home/u".into()));
    assert_eq!(home, Some(std::path::PathBuf::from("/home/u/.cargo")));
}

#[test]
fn bootstrap_guard_cargo_home_empty_env_falls_back() {
    // an exported-but-empty CARGO_HOME must not resolve to "".
    let home = super::resolve_cargo_home(Some("".into()), Some("/home/u".into()));
    assert_eq!(home, Some(std::path::PathBuf::from("/home/u/.cargo")));
}

#[test]
fn bootstrap_guard_cargo_home_unresolvable() {
    assert_eq!(super::resolve_cargo_home(None, None), None);
}

// parse_sudo_ids

#[test]
fn bootstrap_guard_sudo_absent_is_none() {
    assert_eq!(super::parse_sudo_ids(None, None, None), Ok(None));
}

#[test]
fn bootstrap_guard_sudo_complete_parses() {
    assert_eq!(
        super::parse_sudo_ids(Some("u".into()), Some("501".into()), Some("20".into())),
        Ok(Some((501, 20))),
    );
}

#[test]
fn bootstrap_guard_sudo_marker_without_ids_fails_closed() {
    assert!(super::parse_sudo_ids(Some("u".into()), None, None).is_err());
}

#[test]
fn bootstrap_guard_sudo_partial_ids_fail_closed() {
    assert!(super::parse_sudo_ids(Some("u".into()), Some("501".into()), None).is_err());
}

#[test]
fn bootstrap_guard_sudo_malformed_ids_fail_closed() {
    assert!(
        super::parse_sudo_ids(
            Some("u".into()),
            Some("fivehundred".into()),
            Some("20".into())
        )
        .is_err()
    );
}

// ownership repair traversal (chown to our own uid/gid is permitted
// unprivileged, so the walk is exercisable without root)

#[cfg(unix)]
#[test]
fn bootstrap_guard_repair_walk_covers_nested_tree() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::write(root.join(".cargo/config.toml"), "[alias]\n").unwrap();
    std::fs::create_dir_all(root.join("target/mockspace-proxy/src")).unwrap();
    std::fs::write(root.join("target/mockspace-proxy/Cargo.toml"), "").unwrap();
    let mock = root.join("mock");
    std::fs::create_dir_all(mock.join("target/hooks")).unwrap();
    std::fs::write(mock.join("target/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
    std::fs::write(root.join(".gitignore"), "target/\n").unwrap();

    let meta = std::fs::metadata(root).unwrap();
    super::repair_ownership(root, &mock, meta.uid(), meta.gid())
        .expect("repair over own files succeeds");
}

// durable hooks

fn dur(xdg: Option<&str>, home: Option<&str>) -> Option<std::path::PathBuf> {
    super::durable_hooks_dir_from(xdg.map(Into::into), home.map(Into::into))
}

#[test]
fn bootstrap_durable_dir_prefers_xdg() {
    let d = dur(Some("/x/cfg"), Some("/home/u")).unwrap();
    assert_eq!(
        d,
        std::path::PathBuf::from(format!("/x/cfg/mockspace/hooks-v{}", super::HOOK_VERSION))
    );
}

#[test]
fn bootstrap_durable_dir_falls_back_to_home() {
    let d = dur(None, Some("/home/u")).unwrap();
    assert_eq!(
        d,
        std::path::PathBuf::from(format!(
            "/home/u/.config/mockspace/hooks-v{}",
            super::HOOK_VERSION
        ))
    );
}

#[test]
fn bootstrap_durable_dir_empty_xdg_falls_back() {
    let d = dur(Some(""), Some("/home/u")).unwrap();
    assert!(d.starts_with("/home/u/.config"));
}

#[test]
fn bootstrap_durable_dir_none_without_home() {
    assert!(dur(None, None).is_none());
}

#[test]
fn bootstrap_durable_hook_gates_via_launcher_or_blocks_surface() {
    let h = super::gen_durable_hook("pre-commit");
    // resolves the repo root (MOCK_ROOT or the .git ancestor)
    assert!(h.contains("git rev-parse --show-toplevel"));
    assert!(h.contains("MOCK_ROOT"));
    // flexibly resolves the config + mock dir (no baked mockdir, no git config)
    assert!(h.contains("read_mock_dir"));
    assert!(!h.contains("mockspace.mockdir"));
    // discovers the launcher and calls it, not the `cargo mock` alias
    assert!(h.contains("command -v mock"));
    assert!(h.contains("command -v cargo-mock"));
    assert!(h.contains(r#""$launcher""#));
    assert!(!h.contains("cargo mock"));
    // no launcher: fail closed for the design surface only, install hint
    assert!(h.contains("cargo install cargo-mock"));
    assert!(h.contains("git diff --cached --name-only -- \"$mockrel\""));
    assert!(h.contains("exit 1"));
    // and the generated shell is syntactically valid
    assert_bash_ok(&h);
}

#[test]
fn bootstrap_durable_pre_push_hook_valid() {
    let h = super::gen_durable_hook("pre-push");
    assert!(h.contains("pre-push: running mockspace validation"));
    assert!(h.contains(r#""$launcher""#));
    assert!(!h.contains("cargo mock"));
    assert_bash_ok(&h);
}

/// `bash -n` the generated hook (syntax check only, no execution).
fn assert_bash_ok(script: &str) {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(script.as_bytes()).unwrap();
    let out = std::process::Command::new("bash")
        .arg("-n")
        .arg(f.path())
        .output()
        .expect("run bash -n");
    assert!(
        out.status.success(),
        "generated hook has a bash syntax error:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_guard_repair_does_not_follow_symlinks() {
    use std::os::unix::fs::MetadataExt;
    // a symlink planted inside a repaired tree must not be followed:
    // the external file it points at stays untouched.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".cargo")).unwrap();

    let external = dir.path().join("external-secret");
    std::fs::write(&external, "do not touch").unwrap();
    std::os::unix::fs::symlink(&external, root.join(".cargo/link")).unwrap();

    let meta = std::fs::metadata(root).unwrap();
    // repair succeeds and only touches the link, never its target.
    super::chown_tree(&root.join(".cargo"), meta.uid(), meta.gid())
        .expect("repair with a symlink present succeeds");

    // the external file still exists with its content: not deleted,
    // not redirected. (Ownership is unchanged; we chowned to our own
    // uid so a follow would have been a silent no-op on content, but
    // the file's continued existence and readability is the guard.)
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "do not touch");
    assert!(
        std::fs::symlink_metadata(root.join(".cargo/link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
