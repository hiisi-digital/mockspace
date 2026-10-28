//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated hooks find a crate through the project's declared `src_dirs`.
//!
//! Each arm runs the generated bash against a fixture git repository and reads
//! what the hook printed about the scope it chose, and the arguments it passed
//! to `cargo mock`. A stub `cargo` on the path records those arguments and
//! passes, so what the hook said and what it did are both asserted.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::*;

/// The kaski shape: the default directory and a nested group beside it.
const KASKI: &[&str] = &["crates", "applets/console/crates"];

const CONSOLE_SRC: &str = "mock/applets/console/crates/console-core/src/x.rs";

/// Git run in `root`, with the machine's config and identity out of the way
/// (the identity tests' own helper, so the two cannot drift apart), a set
/// author and committer, and the repository-selecting variables a parent git
/// leaves in the environment cleared. A failing call fails the test with its
/// stderr in the message.
fn git(root: &Path, args: &[&str]) -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut c = super::hooks_identity_tests::git_command(
        root,
        &path,
        &[
            ("GIT_AUTHOR_NAME", "t"),
            ("GIT_AUTHOR_EMAIL", "t@example.invalid"),
            ("GIT_COMMITTER_NAME", "t"),
            ("GIT_COMMITTER_EMAIL", "t@example.invalid"),
        ],
        args,
    );
    for k in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"] {
        c.env_remove(k);
    }
    let out = c.output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed with {}:\n{}",
        root.display(),
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
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
        // `cargo` records its arguments, one call to a line, in the file the run
        // names, and passes. The others serve the byline scan.
        for name in ["mock", "cargo-mock", "cargo"] {
            let path = bin.join(name);
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(
                b"#!/usr/bin/env bash\ncat >/dev/null\n\
                  [ -n \"$MS_STUB_ARGS\" ] && echo \"$*\" >> \"$MS_STUB_ARGS\"\nexit 0\n",
            )
            .unwrap();
            f.sync_all().unwrap();
            drop(f);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        bin
    })
}

/// What the hook printed, and the arguments of each `cargo` call it made.
fn run_full(hook_text: &str, root: &Path, stdin: &str) -> (String, Vec<String>) {
    let args_file = root.join("cargo.args");
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
        .env("MS_STUB_ARGS", &args_file)
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
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = fs::read_to_string(&args_file)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    (printed, calls)
}

/// The arguments of the call that carries the scope decision.
fn scope_call(calls: &[String]) -> String {
    let found: Vec<&String> = calls.iter().filter(|c| c.contains("--lint-only")).collect();
    assert_eq!(found.len(), 1, "one scoped call, got {calls:?}");
    found[0].clone()
}

fn run(hook_text: &str, root: &Path, stdin: &str) -> String {
    run_full(hook_text, root, stdin).0
}

/// What the pre-commit hook for `src_dirs` prints with `files` staged, and the
/// arguments it passes to `cargo mock`.
fn commit_run(src_dirs: &[&str], files: &[(&str, &str)]) -> (String, String) {
    commit_run_on(src_dirs, &[], files)
}

/// As `commit_run`, with `base` already committed, so its files are on disk and
/// in the tree without being among the staged ones.
fn commit_run_on(
    src_dirs: &[&str],
    base: &[(&str, &str)],
    files: &[(&str, &str)],
) -> (String, String) {
    let tmp = fixture();
    if !base.is_empty() {
        stage(tmp.path(), base);
        git(tmp.path(), &["commit", "-q", "-m", "base"]);
    }
    stage(tmp.path(), files);
    let (printed, calls) = run_full(
        &gen_pre_commit(
            "mock",
            &HookLayout::of(src_dirs),
            &tmp.path().join("no-user-hook"),
        ),
        tmp.path(),
        "",
    );
    (printed, scope_call(&calls))
}

fn commit_says(src_dirs: &[&str], files: &[(&str, &str)]) -> String {
    commit_run(src_dirs, files).0
}

/// What the pre-push hook for `src_dirs` prints for a push of `files` on top of
/// a base commit, and the arguments it passes to `cargo mock`.
fn push_run(src_dirs: &[&str], files: &[(&str, &str)]) -> (String, String) {
    push_run_on(src_dirs, &[], files)
}

/// As `push_run`, with `base` part of the commit the push is measured against.
fn push_run_on(
    src_dirs: &[&str],
    base: &[(&str, &str)],
    files: &[(&str, &str)],
) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    stage(root, base);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, files);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {base}\n");
    let (printed, calls) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of(src_dirs),
            &root.join("no-user-hook"),
        ),
        root,
        &line,
    );
    (printed, scope_call(&calls))
}

fn push_says(src_dirs: &[&str], files: &[(&str, &str)]) -> String {
    push_run(src_dirs, files).0
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
fn both_configs_are_shared_inputs() {
    for path in ["mock/mockspace.toml", "mockspace.toml"] {
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

const NUKED: &str = "// Nuked by `cargo mock --nuke`\n";

/// Whether the push hook and the engine each call the workspace nuked, over the
/// same tree: `lib_rs` is every crate's `src/lib.rs` body, by path.
fn nuke_verdicts(lib_rs: &[(&str, &str)]) -> (bool, bool) {
    let (out, _) = push_run(KASKI, lib_rs);
    let hook = out.contains("nuked workspace");

    let tmp = fixture();
    for (path, body) in lib_rs {
        let p = tmp.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
    }
    let mock = tmp.path().join("mock");
    let mut cfg = crate::config::Config::from_dir(&mock);
    cfg.src_dirs = vec![mock.join("crates"), mock.join("applets/console/crates")];
    let engine = crate::entry::detect_nuked_workspace(&cfg);
    (hook, engine)
}

const ENGINE_LIB: &str = "mock/crates/kaski-sim/src/lib.rs";
const CONSOLE_LIB: &str = "mock/applets/console/crates/console-core/src/lib.rs";

#[test]
fn a_workspace_with_every_crate_nuked_is_nuked_for_the_hook_and_the_engine() {
    let (hook, engine) = nuke_verdicts(&[(ENGINE_LIB, NUKED), (CONSOLE_LIB, NUKED)]);
    assert!(engine, "the engine calls it nuked");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

/// One nuked console crate beside a live engine crate is not a nuked workspace.
/// The hook took any crate for all, and turned the source lints off for the live
/// engine crate.
#[test]
fn one_nuked_crate_beside_a_live_one_is_not_nuked_for_the_hook_or_the_engine() {
    let (hook, engine) = nuke_verdicts(&[(ENGINE_LIB, "fn s() {}\n"), (CONSOLE_LIB, NUKED)]);
    assert!(!engine, "the engine calls it live");
    assert_eq!(hook, engine, "the hook and the engine disagree");
}

#[test]
fn the_nuke_marker_is_the_configured_one() {
    let root = fixture();
    stage(root.path(), &[
        ("README", "base\n"),
        (CONSOLE_LIB, "// Nuked by a design round\n"),
    ]);
    git(root.path(), &["commit", "-q", "-m", "base"]);
    let base = git(root.path(), &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {base} refs/heads/m {}\n", "1".repeat(40));
    let layout = HookLayout {
        nuke_marker: "Nuked by a design round".to_string(),
        ..HookLayout::of(&["applets/console/crates"])
    };
    let (out, _) = run_full(
        &gen_pre_push("mock", &layout, &root.path().join("no-user-hook")),
        root.path(),
        &line,
    );
    assert!(out.contains("nuked workspace"), "hook said:\n{out}");
    // the same tree under the default marker is live
    let (out, _) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of(&["applets/console/crates"]),
            &root.path().join("no-user-hook"),
        ),
        root.path(),
        &line,
    );
    assert!(!out.contains("nuked workspace"), "hook said:\n{out}");
}

/// The kaski shape on disk: the mock workspace and the console workspace, each
/// with its manifest and lockfile and its own crates.
const KASKI_TREE: &[(&str, &str)] = &[
    ("mock/Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
    ("mock/Cargo.lock", "# lock 1\n"),
    ("mock/crates/kaski-sim/src/lib.rs", "fn s() {}\n"),
    ("mock/crates/kaski-ui/src/lib.rs", "fn u() {}\n"),
    (
        "mock/applets/console/Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    ),
    ("mock/applets/console/Cargo.lock", "# lock 1\n"),
    (
        "mock/applets/console/crates/console-core/src/lib.rs",
        "fn c() {}\n",
    ),
    (
        "mock/applets/console/crates/console-serve/src/lib.rs",
        "fn v() {}\n",
    ),
    (
        "mock/applets/console/crates/console-app/src/lib.rs",
        "fn a() {}\n",
    ),
];

const CONSOLE_SCOPE: &str = "console-app,console-core,console-serve";

#[test]
fn the_console_lockfile_alone_scopes_to_the_console_crates() {
    let staged = [("mock/applets/console/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --strict --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn the_console_manifest_alone_scopes_to_the_console_crates() {
    let staged = [(
        "mock/applets/console/Cargo.toml",
        "[workspace]\nmembers = []\n",
    )];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn console_source_and_its_lockfile_scope_to_all_three_crates() {
    let staged = [
        ("mock/applets/console/Cargo.lock", "# lock 2\n"),
        (
            "mock/applets/console/crates/console-core/src/x.rs",
            "fn x() {}\n",
        ),
    ];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --commit --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args,
        format!("mock --lint-only --strict --scope {CONSOLE_SCOPE}"),
        "{out}"
    );
}

#[test]
fn the_mock_lockfile_scopes_to_the_mock_workspace_and_not_the_console() {
    let staged = [("mock/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args, "mock --lint-only --commit --scope kaski-sim,kaski-ui",
        "{out}"
    );
    let (out, args) = push_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(
        args, "mock --lint-only --strict --scope kaski-sim,kaski-ui",
        "{out}"
    );
}

/// The control for the arms above: a crate's own lockfile is not a workspace root,
/// so it moves no other crate. It is that crate's source, since a changed
/// dependency is code the run compiles, so the scope keeps the source lints on.
#[test]
fn a_crates_own_lockfile_is_that_crates_source_and_moves_no_workspace() {
    let staged = [("mock/crates/kaski-sim/Cargo.lock", "# lock 2\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim", "{out}");
}

/// A `[features]` or `[lints]` edit lives in the crate's own manifest, and a
/// `--doc-only` run skips the very source lints that edit changes.
#[test]
fn a_crates_own_manifest_is_that_crates_source_not_doc_only() {
    let staged = [(
        "mock/applets/console/crates/console-core/Cargo.toml",
        "[package]\nname = \"console-core\"\n[features]\nx = []\n",
    )];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit --scope console-core", "{out}");
}

/// The control: a documentation file of the same crate is still doc-only, so the
/// two arms above are about the manifest and not about the flag being gone.
#[test]
fn a_crates_documentation_alone_is_still_doc_only() {
    let docs = [("mock/crates/kaski-sim/DESIGN.md.tmpl", "x\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &docs);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim --doc-only", "{out}");
}

/// A shared input staged beside a workspace lockfile keeps `--doc-only` off: the
/// lockfile is a dependency the run has to compile. Without the guard on the
/// workspace crates the shared-input arm adds the flag back.
#[test]
fn a_shared_input_beside_a_workspace_lockfile_is_not_doc_only() {
    let staged = [
        ("mock/registry/tenets.toml", "x = 1\n"),
        ("mock/Cargo.lock", "# lock 2\n"),
    ];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &staged);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    // and the shared input alone, over the same tree, is doc-only
    let alone = [("mock/registry/tenets.toml", "x = 1\n")];
    let (out, args) = commit_run_on(KASKI, KASKI_TREE, &alone);
    assert_eq!(args, "mock --lint-only --commit --doc-only", "{out}");
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
fn the_declaration_is_the_one_the_source_layout_holds() {
    let mock = Path::new("/r/mock");
    let dirs = [
        mock.join("crates"),
        mock.join("applets/console/crates/"),
        Path::new("/elsewhere/crates").to_path_buf(),
        mock.to_path_buf(),
    ];
    let config = Path::new("/r/mock/mockspace.toml");
    let hook = HookLayout::declared(Path::new("/r"), mock, &dirs, "m", config);
    // The entries the layout can address are kept and the
    // ones it cannot are not. An entry with a `..` in it never gets here,
    // `Config::from_dir` refuses it.
    assert_eq!(hook.src_dirs, vec![
        "crates".to_string(),
        "applets/console/crates".to_string()
    ]);
}

#[test]
fn the_generated_files_follow_the_declaration() {
    // The fingerprint guard skips a rewrite when the content is unchanged, so a
    // declaration that changes must change the content or the old hook stays.
    let a = gen_pre_commit("mock", &HookLayout::of(&["crates"]), Path::new("/dev/null"));
    let b = gen_pre_commit("mock", &HookLayout::of(KASKI), Path::new("/dev/null"));
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
        crate::config::DEFAULT_NUKE_MARKER,
        &root.join("mockspace.toml"),
        &mut actions,
    );
    let hook = fs::read_to_string(generated_hooks_dir(&mock).join("pre-commit")).unwrap();
    assert!(
        hook.contains("SRC_DIRS=('crates' 'applets/console/crates')"),
        "{hook}"
    );
}

/// The labels the hook echoes and the arguments it hands the launcher are two
/// things, and only the second decides what runs. Each branch is asserted on the
/// arguments the stub `cargo` recorded.
#[test]
fn the_commit_branches_pass_these_arguments() {
    let table: &[(&str, &[(&str, &str)], &str)] = &[
        (
            "one crate",
            &[(CONSOLE_SRC, "fn x() {}\n")],
            "mock --lint-only --commit --scope console-core",
        ),
        (
            "doc-only crate",
            &[(
                "mock/applets/console/crates/console-core/DESIGN.md.tmpl",
                "x\n",
            )],
            "mock --lint-only --commit --scope console-core --doc-only",
        ),
        (
            "no crate",
            &[("mock/notes.md", "x\n")],
            "mock --lint-only --commit --scope infra",
        ),
        (
            "shared input beside source",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit",
        ),
        (
            "shared input alone",
            &[("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --commit --doc-only",
        ),
    ];
    for (what, files, want) in table {
        let (out, args) = commit_run(KASKI, files);
        assert_eq!(args, *want, "{what}: hook said:\n{out}");
    }
}

#[test]
fn the_push_branches_pass_these_arguments() {
    let table: &[(&str, &[(&str, &str)], &str)] = &[
        (
            "one crate",
            &[(CONSOLE_SRC, "fn x() {}\n")],
            "mock --lint-only --strict --scope console-core",
        ),
        (
            "no crate",
            &[("README.md", "x\n")],
            "mock --lint-only --strict",
        ),
        (
            "shared input beside source",
            &[(CONSOLE_SRC, "fn x() {}\n"), ("mock/registry/tenets.toml", "x = 1\n")],
            "mock --lint-only --strict",
        ),
    ];
    for (what, files, want) in table {
        let (out, args) = push_run(KASKI, files);
        assert_eq!(args, *want, "{what}: hook said:\n{out}");
    }
}

/// A `mockspace.toml` a crate keeps as a test fixture is that crate's file, and
/// the config is named only where it lives: the repo root and the mock directory.
#[test]
fn a_fixture_named_like_the_config_stays_scoped_to_its_crate() {
    let fixture = "mock/applets/console/crates/console-core/tests/fixture/kaski/mockspace.toml";
    let (out, args) = commit_run(KASKI, &[(CONSOLE_SRC, "fn x() {}\n"), (fixture, "x = 1\n")]);
    assert_eq!(
        args, "mock --lint-only --commit --scope console-core",
        "{out}"
    );
    let (out, args) = push_run(KASKI, &[(CONSOLE_SRC, "fn x() {}\n"), (fixture, "x = 1\n")]);
    assert_eq!(
        args, "mock --lint-only --strict --scope console-core",
        "{out}"
    );
}

/// A project lint is compiled into the run, so a commit that edits one and no
/// crate must not be scoped to `infra`, where the changed lint never runs.
#[test]
fn a_project_lint_is_a_shared_input() {
    let (out, args) = commit_run(KASKI, &[("mock/lints/no-foo/src/lib.rs", "fn l() {}\n")]);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    let (out, args) = push_run(KASKI, &[
        (CONSOLE_SRC, "fn x() {}\n"),
        ("mock/lints/no-foo/src/lib.rs", "fn l() {}\n"),
    ]);
    assert_eq!(args, "mock --lint-only --strict", "{out}");
}

const PACK: &str = "mock/crates/kaski-asset-lints";

fn commit_with_packs(
    packs: &[&str],
    base: &[(&str, &str)],
    files: &[(&str, &str)],
) -> (String, String) {
    let tmp = fixture();
    if !base.is_empty() {
        stage(tmp.path(), base);
        git(tmp.path(), &["commit", "-q", "-m", "base"]);
    }
    stage(tmp.path(), files);
    let (printed, calls) = run_full(
        &gen_pre_commit(
            "mock",
            &HookLayout::of_with_packs(KASKI, packs),
            &tmp.path().join("no-user-hook"),
        ),
        tmp.path(),
        "",
    );
    (printed, scope_call(&calls))
}

fn push_with_packs(packs: &[&str], files: &[(&str, &str)]) -> (String, String) {
    let tmp = fixture();
    let root = tmp.path();
    stage(root, &[("README", "base\n")]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    stage(root, files);
    git(root, &["commit", "-q", "-m", "work"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let line = format!("refs/heads/m {head} refs/heads/m {base}\n");
    let (printed, calls) = run_full(
        &gen_pre_push(
            "mock",
            &HookLayout::of_with_packs(KASKI, packs),
            &root.join("no-user-hook"),
        ),
        root,
        &line,
    );
    (printed, scope_call(&calls))
}

/// A lint pack declared by path is code the run compiles, as a file under the mock
/// directory's `lints/` is. One that sits under a source directory is also a crate,
/// and the crate reading would bound the commit to it, so the changed lint would
/// never run over the others.
#[test]
fn a_pack_under_a_source_directory_is_a_shared_input() {
    let src = format!("{PACK}/src/lib.rs");
    let manifest = format!("{PACK}/Cargo.toml");
    for file in [&src, &manifest] {
        let (out, args) = commit_with_packs(&[PACK], &[], &[(file, "x\n")]);
        assert_eq!(args, "mock --lint-only --commit", "{file}: {out}");
        let (out, args) = push_with_packs(&[PACK], &[(file, "x\n")]);
        assert_eq!(args, "mock --lint-only --strict", "{file}: {out}");
    }
}

/// The control: a sibling crate in the same source directory stays bounded to
/// itself, so the arm above is about the pack and not about the directory.
#[test]
fn a_crate_beside_a_pack_stays_scoped_to_itself() {
    let sibling = "mock/crates/kaski-sim/src/lib.rs";
    let (out, args) = commit_with_packs(&[PACK], &[], &[(sibling, "fn s() {}\n")]);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-sim", "{out}");
    let (out, args) = push_with_packs(&[PACK], &[(sibling, "fn s() {}\n")]);
    assert_eq!(args, "mock --lint-only --strict --scope kaski-sim", "{out}");
}

/// A pack that is declared but has not been staged leaves a commit as before.
#[test]
fn the_same_edit_without_a_declared_pack_is_scoped_to_its_crate() {
    let src = format!("{PACK}/src/lib.rs");
    let (out, args) = commit_with_packs(&[], &[], &[(&src, "fn l() {}\n")]);
    assert_eq!(args, "mock --lint-only --commit --scope kaski-asset-lints", "{out}");
}

/// A pack outside the mock directory is staged outside the surface the hook looks
/// at, and still enters the gate with the full scope.
#[test]
fn a_pack_outside_the_mock_directory_enters_the_gate() {
    let (out, args) = commit_with_packs(&["tools/packs/p"], &[], &[(
        "tools/packs/p/src/lib.rs",
        "fn l() {}\n",
    )]);
    assert_eq!(args, "mock --lint-only --commit", "{out}");
    let (out, args) = push_with_packs(&["tools/packs/p"], &[(
        "tools/packs/p/src/lib.rs",
        "fn l() {}\n",
    )]);
    assert_eq!(args, "mock --lint-only --strict", "{out}");
}

/// The packs the layout carries are those the config declares by path, resolved the
/// way cargo reads them: from the generated lint crate at
/// `<mock>/target/mockspace-lints/`, and kept as repository-relative directories.
#[test]
fn the_layout_reads_the_path_packs_of_the_config() {
    let tmp = fixture();
    let root = tmp.path();
    let mock = root.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let config = root.join("mockspace.toml");
    fs::write(
        &config,
        "[lint-crates]\n\
         remote = { git = \"ssh://git@github.com/o/r.git\", branch = \"dev\" }\n\
         version = \"0.1\"\n\
         inline = { path = \"../../crates/kaski-asset-lints\" }\n\
         outside = { path = \"../../../../elsewhere\" }\n\
         [lint-crates.table]\n\
         path = \"../../../tools/table-pack/\"\n",
    )
    .unwrap();
    let layout = HookLayout::declared(root, &mock, &[mock.join("crates")], "m", &config);
    assert_eq!(layout.lint_packs, vec![
        "mock/crates/kaski-asset-lints".to_string(),
        "tools/table-pack".to_string(),
    ]);
    // no config, no packs
    let none = HookLayout::declared(root, &mock, &[], "m", &root.join("absent.toml"));
    assert!(none.lint_packs.is_empty());
}
