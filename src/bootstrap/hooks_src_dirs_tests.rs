//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated hooks find a crate through the project's declared `src_dirs`.
//!
//! Each arm runs the generated bash against a fixture git repository and reads
//! what the hook printed about the scope it chose. `cargo mock` is not on the
//! path in these runs and the hook fails when it reaches it, which is after
//! everything asserted here has been printed.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::*;

/// The kaski shape: the default directory and a nested group beside it.
const KASKI: &[&str] = &["crates", "applets/console/crates"];

const CONSOLE_SRC: &str = "mock/applets/console/crates/console-core/src/x.rs";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    git(tmp.path(), &["config", "user.email", "t@example.invalid"]);
    git(tmp.path(), &["config", "user.name", "t"]);
    tmp
}

fn stage(root: &Path, files: &[(&str, &str)]) {
    for (path, body) in files {
        let p = root.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        git(root, &["add", path]);
    }
}

/// A directory holding stubs of the launcher that pass, so the pre-push byline
/// scan gets through to the scope decision this file is about, whether or not
/// the machine running the test has a real one.
///
/// The stub drains its stdin before it exits. The scan pipes every commit
/// message into the launcher under `pipefail`, so a stub that exits without
/// reading is killed on the other end by SIGPIPE and the hook ends silently,
/// which is a refusal that looks like an empty output. Written once per process.
fn stub_bin() -> &'static Path {
    static BIN: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let bin = std::env::temp_dir().join(format!("ms_hook_stubs_{}", std::process::id()));
        fs::create_dir_all(&bin).unwrap();
        for name in ["mock", "cargo-mock"] {
            let path = bin.join(name);
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(b"#!/usr/bin/env bash\ncat >/dev/null\nexit 0\n")
                .unwrap();
            f.sync_all().unwrap();
            drop(f);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        bin
    })
}

fn run(hook_text: &str, root: &Path, stdin: &str) -> String {
    let hook = root.join("hook.sh");
    fs::write(&hook, hook_text).unwrap();
    let bin = stub_bin();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new("bash")
        .arg(&hook)
        .current_dir(root)
        .env("PATH", path)
        .env("HOME", root)
        .env_remove("XDG_CONFIG_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("bash runs");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// What the pre-commit hook for `src_dirs` prints with `files` staged.
fn commit_says(src_dirs: &[&str], files: &[(&str, &str)]) -> String {
    let tmp = fixture();
    stage(tmp.path(), files);
    run(
        &gen_pre_commit("mock", src_dirs, &tmp.path().join("no-user-hook")),
        tmp.path(),
        "",
    )
}

/// What the pre-push hook for `src_dirs` prints for a push of `files` on top of
/// a base commit.
fn push_says(src_dirs: &[&str], files: &[(&str, &str)]) -> String {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, files);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {base}\n");
    run(
        &gen_pre_push("mock", src_dirs, &root.join("no-user-hook")),
        root,
        &line,
    )
}

#[test]
fn source_under_the_second_source_directory_scopes_to_its_crate() {
    let out = commit_says(KASKI, &[(CONSOLE_SRC, "fn x() {}\n")]);
    assert!(out.contains("crates: console-core"), "hook said:\n{out}");
    assert!(
        !out.contains("infrastructure-only"),
        "crate lints must run over staged source, not be scoped to `infra`:\n{out}"
    );
    assert!(
        !out.contains("(doc-only)"),
        "staged `.rs` is source:\n{out}"
    );
}

/// The control for the arm above. A hook generated from the default declaration
/// alone is what the old literal `crates` amounted to, and it must fail the very
/// assertions the arm above makes, or those assertions prove nothing.
#[test]
fn the_old_literal_fails_the_arm_above() {
    let out = commit_says(&["crates"], &[(CONSOLE_SRC, "fn x() {}\n")]);
    assert!(
        out.contains("infrastructure-only"),
        "a hook that only knows `crates` scopes the console crate to `infra`:\n{out}"
    );
    assert!(!out.contains("crates: console-core"), "hook said:\n{out}");
}

#[test]
fn crates_under_both_directories_are_scoped_together() {
    let out = commit_says(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        ("mock/crates/kaski-sim/src/lib.rs", "fn s() {}\n"),
    ]);
    assert!(
        out.contains("crates: console-core,kaski-sim"),
        "hook said:\n{out}"
    );
}

#[test]
fn a_doc_only_change_in_the_second_directory_stays_doc_only() {
    let out = commit_says(KASKI, &[(
        "mock/applets/console/crates/console-core/DESIGN.md.tmpl",
        "x\n",
    )]);
    assert!(
        out.contains("crates: console-core (doc-only)"),
        "hook said:\n{out}"
    );
}

#[test]
fn a_directory_that_is_not_declared_is_not_source() {
    // The other end of the declaration: `applets/console/crates` is source only
    // because the project said so.
    let out = commit_says(&["crates"], &[(
        "mock/applets/console/crates/console-core/DESIGN.md.tmpl",
        "x\n",
    )]);
    assert!(out.contains("infrastructure-only"), "hook said:\n{out}");
}

#[test]
fn a_staged_registry_row_beside_a_crate_runs_everything() {
    let out = commit_says(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        ("mock/registry/tenets.toml", "[[tenet]]\nid = \"a\"\n"),
    ]);
    assert!(
        out.contains("shared inputs staged: full scope"),
        "hook said:\n{out}"
    );
    assert!(
        !out.contains("crates: console-core"),
        "a shared input is not bounded to the crate beside it:\n{out}"
    );
    assert!(!out.contains("(doc-only)"), "source is staged:\n{out}");
}

#[test]
fn a_staged_registry_row_alone_runs_everything_and_not_infra() {
    let out = commit_says(KASKI, &[(
        "mock/registry/tenets.toml",
        "[[tenet]]\nid = \"a\"\n",
    )]);
    assert!(
        out.contains("shared inputs staged: full scope"),
        "hook said:\n{out}"
    );
    assert!(!out.contains("infrastructure-only"), "hook said:\n{out}");
}

#[test]
fn the_workspace_manifest_and_lockfile_and_both_configs_are_shared_inputs() {
    for path in ["mock/Cargo.toml", "mock/Cargo.lock", "mock/mockspace.toml", "mockspace.toml"] {
        let out = commit_says(KASKI, &[(CONSOLE_SRC, "fn x() {}\n"), (path, "x = 1\n")]);
        assert!(
            out.contains("shared inputs staged: full scope"),
            "{path}: hook said:\n{out}"
        );
    }
}

/// The control for the shared-input arms: a crate's own manifest is not shared,
/// so it stays bounded to its crate.
#[test]
fn a_crates_own_manifest_is_not_a_shared_input() {
    let out = commit_says(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        (
            "mock/applets/console/crates/console-core/Cargo.toml",
            "[package]\n",
        ),
    ]);
    assert!(out.contains("crates: console-core"), "hook said:\n{out}");
    assert!(!out.contains("shared inputs staged"), "hook said:\n{out}");
}

#[test]
fn a_push_touching_the_second_directory_is_scoped_to_its_crate() {
    let out = push_says(KASKI, &[(CONSOLE_SRC, "fn x() {}\n")]);
    assert!(out.contains("scope: console-core"), "hook said:\n{out}");
}

#[test]
fn the_old_literal_pushes_the_console_crate_unscoped_by_name() {
    let out = push_says(&["crates"], &[(CONSOLE_SRC, "fn x() {}\n")]);
    assert!(!out.contains("scope: console-core"), "hook said:\n{out}");
    assert!(out.contains("no crate changes"), "hook said:\n{out}");
}

#[test]
fn a_push_with_a_registry_row_beside_a_crate_runs_the_full_scope() {
    let out = push_says(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        ("mock/registry/tenets.toml", "[[tenet]]\nid = \"a\"\n"),
    ]);
    assert!(out.contains("scope: full project"), "hook said:\n{out}");
    assert!(!out.contains("scope: console-core"), "hook said:\n{out}");
}

#[test]
fn a_nuked_crate_under_the_second_directory_is_found_by_the_push_hook() {
    let out = push_says(KASKI, &[(
        "mock/applets/console/crates/console-core/src/lib.rs",
        "// Nuked by a design round\n",
    )]);
    assert!(out.contains("nuked workspace"), "hook said:\n{out}");
}

#[test]
fn an_empty_declaration_is_a_documentation_repository() {
    let out = commit_says(&[], &[(CONSOLE_SRC, "fn x() {}\n")]);
    assert!(out.contains("infrastructure-only"), "hook said:\n{out}");
}

#[test]
fn a_directory_with_a_quote_in_it_cannot_break_the_script() {
    let out = commit_says(&["it's/crates"], &[(
        "mock/it's/crates/odd/src/lib.rs",
        "fn x() {}\n",
    )]);
    assert!(out.contains("crates: odd"), "hook said:\n{out}");
}

#[test]
fn the_declaration_is_written_relative_to_the_mock_directory() {
    let mock = Path::new("/r/mock");
    let dirs = [
        mock.join("crates"),
        mock.join("applets/console/crates/"),
        mock.join("../outside"),
        Path::new("/elsewhere/crates").to_path_buf(),
        mock.to_path_buf(),
    ];
    assert_eq!(src_dirs_relative(mock, &dirs), vec![
        "crates".to_string(),
        "applets/console/crates".to_string()
    ]);
}

#[test]
fn the_generated_files_follow_the_declaration() {
    // The fingerprint guard skips a rewrite when the content is unchanged, so a
    // declaration that changes must change the content or the old hook stays.
    let a = gen_pre_commit("mock", &["crates"], Path::new("/dev/null"));
    let b = gen_pre_commit("mock", KASKI, Path::new("/dev/null"));
    assert_ne!(content_fingerprint(&a), content_fingerprint(&b));
}

#[test]
fn ensure_generated_hooks_writes_the_declared_directories() {
    let tmp = fixture();
    let root = tmp.path();
    let mock = root.join("mock");
    fs::create_dir_all(mock.join("applets/console/crates")).unwrap();
    fs::create_dir_all(mock.join("crates")).unwrap();
    let mut actions = Vec::new();
    ensure_generated_hooks(
        root,
        &mock,
        &[mock.join("crates"), mock.join("applets/console/crates")],
        &mut actions,
    );
    let hook = fs::read_to_string(generated_hooks_dir(&mock).join("pre-commit")).unwrap();
    assert!(
        hook.contains("SRC_DIRS=('crates' 'applets/console/crates')"),
        "{hook}"
    );
}
