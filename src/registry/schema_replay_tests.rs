//! A schema check over the inputs it already judged is replayed, and over any input that moved it
//! runs again. A stand-in for taplo counts its runs, so "replayed" is observed and not assumed.

#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::super::validate::{SchemaCheck, check_schemas_with};
use super::super::{RegistryNamespace, builtin_vocab};
use crate::lint::verdicts::DiskVerdicts;
use crate::render_design::GENERATED_MARKER;

/// What mockspace writes at the root: marked as generated, with the registry as its include.
const GENERATED_CONFIG: &str = "# AUTO-GENERATED: DO NOT EDIT DIRECTLY\n\n[[rule]]\ninclude = [\"mock/registry/**/*.toml\"]\nschema.path = \"target/mockspace/registry-schemas/registry.schema.json\"\n";

/// Held by every check, so a test that sets an environment variable is not seen by another.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A repository root with a registry file and a taplo stand-in that counts the times it checked.
struct Fixture {
    root:  TempDir,
    store: TempDir,
    bin:   TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self { root: TempDir::new().unwrap(), store: TempDir::new().unwrap(), bin: TempDir::new().unwrap() };
        fs::create_dir_all(f.root.path().join("mock/registry")).unwrap();
        fs::write(f.root.path().join("mock/registry/a.toml"), "[[vocab]]\nid = \"a\"\n").unwrap();
        fs::write(f.root.path().join(".taplo.toml"), GENERATED_CONFIG).unwrap();
        fs::create_dir_all(f.root.path().join("target/mockspace/registry-schemas")).unwrap();
        fs::write(f.root.path().join("target/mockspace/registry-schemas/registry.schema.json"), "{}").unwrap();
        f.taplo(0, 2, "taplo 0.10.0");
        f
    }

    fn taplo_path(&self) -> PathBuf {
        self.bin.path().join("taplo")
    }

    /// Install the stand-in: it exits with `exit`, reports `total` files found, prints `version`.
    fn taplo(&self, exit: u8, total: u32, version: &str) {
        let dir = self.bin.path();
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '{version}'; exit 0; fi\n\
             echo run >> '{runs}'\necho 'found files total={total} excluded=0' >&2\n\
             [ {exit} -ne 0 ] && echo 'error: mock/registry/a.toml does not match' >&2\nexit {exit}\n",
            runs = dir.join("runs").display(),
        );
        let path = self.taplo_path();
        let _ = fs::remove_file(&path);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn runs(&self) -> usize {
        fs::read_to_string(self.bin.path().join("runs")).map_or(0, |t| t.lines().count())
    }

    fn check(&self, program: &Path) -> SchemaCheck {
        let _serial = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let store = DiskVerdicts::open_in(self.store.path(), "pack").expect("a store");
        let namespaces: Vec<RegistryNamespace> = vec![builtin_vocab()];
        check_schemas_with(&OsString::from(program), self.root.path(), &namespaces, Some(&store))
    }

    fn passes(&self) -> bool {
        matches!(self.check(&self.taplo_path()), SchemaCheck::Ran { failures } if failures.is_empty())
    }
}

#[test]
fn an_unchanged_tree_replays_the_pass_and_does_not_run_taplo_again() {
    let f = Fixture::new();
    assert!(f.passes());
    assert_eq!(f.runs(), 1, "the control: a first run asks taplo");
    assert!(f.passes());
    assert!(f.passes());
    assert_eq!(f.runs(), 1, "the same inputs are answered from the store");
}

#[test]
fn a_changed_registry_file_is_judged_again() {
    let f = Fixture::new();
    assert!(f.passes());
    fs::write(f.root.path().join("mock/registry/a.toml"), "[[vocab]]\nid = \"b\"\n").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "a moved registry file runs taplo again");
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "and the new tree replays in its turn");
}

#[test]
fn a_new_registry_file_is_judged_again() {
    let f = Fixture::new();
    assert!(f.passes());
    fs::write(f.root.path().join("mock/registry/b.toml"), "[[vocab]]\nid = \"c\"\n").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2);
}

#[test]
fn a_changed_taplo_config_schema_or_taplo_version_is_judged_again() {
    let f = Fixture::new();
    assert!(f.passes());
    fs::write(f.root.path().join(".taplo.toml"), format!("{GENERATED_CONFIG}# moved\n")).unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "the config moved");
    fs::write(f.root.path().join("target/mockspace/registry-schemas/registry.schema.json"), "{\"x\":1}").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 3, "the schema moved");
    f.taplo(0, 2, "taplo 0.11.0");
    assert!(f.passes());
    assert_eq!(f.runs(), 4, "taplo itself moved");
}

#[test]
fn a_failure_is_never_replayed_as_a_pass() {
    let f = Fixture::new();
    f.taplo(1, 2, "taplo 0.10.0");
    assert!(!f.passes());
    assert!(!f.passes());
    assert_eq!(f.runs(), 2, "a failing tree runs taplo each time and keeps failing");
}

#[test]
fn a_check_that_examined_no_files_is_never_remembered() {
    let f = Fixture::new();
    f.taplo(0, 0, "taplo 0.10.0");
    assert!(!f.passes(), "zero files examined is a failure");
    assert!(!f.passes());
    assert_eq!(f.runs(), 2, "and is judged again, not replayed");
}

#[test]
fn an_absent_taplo_is_unavailable_and_a_replay_does_not_cover_for_it() {
    let f = Fixture::new();
    assert!(f.passes());
    let missing = f.bin.path().join("no-such-taplo");
    assert!(matches!(f.check(&missing), SchemaCheck::Unavailable), "an earlier pass does not make a missing taplo pass");
    assert_eq!(f.runs(), 1);
}

#[test]
fn the_generated_config_in_the_fixture_is_the_marked_one() {
    assert!(GENERATED_CONFIG.contains(GENERATED_MARKER));
}

#[test]
fn a_toml_outside_the_include_that_moves_is_judged_again() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.path().join("elsewhere/target")).unwrap();
    fs::write(f.root.path().join("elsewhere/x.toml"), "a = 1\n").unwrap();
    fs::write(f.root.path().join("elsewhere/target/y.toml"), "a = 1\n").unwrap();
    assert!(f.passes());
    fs::write(f.root.path().join("elsewhere/x.toml"), "a = 2\n").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "a toml taplo might read moved");
    fs::write(f.root.path().join("elsewhere/target/y.toml"), "a = 2\n").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 3, "only the root target directory is left out, not every one");
}

#[test]
fn a_config_the_project_wrote_is_never_replayed() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.path().join("schemas")).unwrap();
    fs::write(f.root.path().join("schemas/custom.json"), "{}").unwrap();
    fs::write(
        f.root.path().join(".taplo.toml"),
        "[[rule]]\ninclude = [\"mock/registry/**/*.toml\"]\nschema.path = \"schemas/custom.json\"\n",
    )
    .unwrap();
    assert!(f.passes());
    fs::write(f.root.path().join("schemas/custom.json"), "{\"changed\":true}").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "a schema the key does not hold moved, so the check ran again");
}

#[test]
fn a_second_taplo_config_is_never_replayed() {
    let f = Fixture::new();
    fs::write(f.root.path().join("taplo.toml"), "include = [\"**/*.toml\"]\n").unwrap();
    assert!(f.passes());
    assert!(f.passes());
    assert_eq!(f.runs(), 2);
}

#[test]
fn a_registry_file_naming_its_own_schema_is_never_replayed() {
    let f = Fixture::new();
    fs::write(f.root.path().join("s.json"), "{}").unwrap();
    fs::write(f.root.path().join("mock/registry/a.toml"), "#:schema ../../s.json\n[[vocab]]\nid = \"a\"\n").unwrap();
    assert!(f.passes());
    fs::write(f.root.path().join("s.json"), "{\"changed\":true}").unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2);
}

#[test]
fn an_environment_that_names_another_config_gives_no_key() {
    let f = Fixture::new();
    let store = DiskVerdicts::open_in(f.store.path(), "pack").expect("a store");
    assert!(super::input_key(&store, f.root.path(), b"taplo 0.10.0", false).is_some(), "the control: a plain tree has a key");
    assert!(super::input_key(&store, f.root.path(), b"taplo 0.10.0", true).is_none());
}

#[test]
fn a_taplo_that_prints_its_version_on_stderr_keys_on_it() {
    let f = Fixture::new();
    let path = f.taplo_path();
    let body = |version: &str| {
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '{version}' >&2; exit 0; fi\n\
             echo run >> '{runs}'\necho 'found files total=2 excluded=0' >&2\nexit 0\n",
            runs = f.bin.path().join("runs").display()
        )
    };
    let install = |version: &str| {
        let _ = fs::remove_file(&path);
        fs::write(&path, body(version)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    install("taplo 0.10.0");
    assert!(f.passes());
    install("taplo 0.11.0");
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "the version on stderr moved");
}

/// A root as mockspace writes it: the generated config, one valid registry file, the schema.
fn real_root() -> TempDir {
    let root = TempDir::new().unwrap();
    let p = root.path();
    fs::create_dir_all(p.join("mock/registry")).unwrap();
    fs::create_dir_all(p.join("target/mockspace/registry-schemas")).unwrap();
    fs::write(p.join("mock/registry/a.toml"), "[[vocab]]\nid = \"a\"\n").unwrap();
    fs::write(p.join("target/mockspace/registry-schemas/registry.schema.json"), "{}").unwrap();
    fs::write(
        p.join(".taplo.toml"),
        format!(
            "# {GENERATED_MARKER}\n\n[[rule]]\ninclude = [\"mock/registry/**/*.toml\"]\nschema.path = \"target/mockspace/registry-schemas/registry.schema.json\"\n"
        ),
    )
    .unwrap();
    root
}

fn taplo_installed() -> bool {
    std::process::Command::new("taplo").arg("--version").output().is_ok()
}

fn real_check(root: &Path, store: &Path) -> bool {
    let store = DiskVerdicts::open_in(store, "pack").expect("a store");
    let namespaces: Vec<RegistryNamespace> = vec![builtin_vocab()];
    matches!(
        check_schemas_with(&OsString::from("taplo"), root, &namespaces, Some(&store)),
        SchemaCheck::Ran { failures } if failures.is_empty()
    )
}

/// Pass once with real taplo, break something taplo reads, and expect the next check to fail.
fn breaks_after_a_pass(prepare: impl Fn(&Path), break_it: impl Fn(&Path)) {
    assert!(taplo_installed(), "these tests run the real taplo; put it on PATH (the bootstrap installs it)");
    let (root, store) = (real_root(), TempDir::new().unwrap());
    prepare(root.path());
    assert!(real_check(root.path(), store.path()), "the control: the tree passes real taplo");
    assert!(real_check(root.path(), store.path()), "and replays");
    break_it(root.path());
    assert!(!real_check(root.path(), store.path()), "real taplo fails the broken tree, so a replay must not pass it");
}

#[test]
fn real_taplo_a_broken_toml_under_the_root_target_is_not_replayed_as_a_pass() {
    breaks_after_a_pass(|_| {}, |p| fs::write(p.join("target/broken.toml"), "bad = =\n").unwrap());
}

#[test]
fn real_taplo_a_broken_toml_under_dot_git_is_not_replayed_as_a_pass() {
    breaks_after_a_pass(
        |p| fs::create_dir_all(p.join(".git")).unwrap(),
        |p| fs::write(p.join(".git/broken.toml"), "bad = =\n").unwrap(),
    );
}

#[test]
fn real_taplo_a_schema_bound_by_a_root_dollar_schema_key_is_not_replayed() {
    breaks_after_a_pass(
        |p| {
            fs::create_dir_all(p.join("other")).unwrap();
            fs::write(p.join("other/s.json"), "{}").unwrap();
            fs::write(p.join("other/x.toml"), "\"$schema\" = \"./s.json\"\nname = \"x\"\n").unwrap();
        },
        |p| fs::write(p.join("other/s.json"), "{\"type\":\"object\",\"required\":[\"need\"]}").unwrap(),
    );
}

#[test]
fn real_taplo_a_symlinked_directory_of_tomls_is_not_replayed() {
    let outside = TempDir::new().unwrap();
    let outside_path = outside.path().to_path_buf();
    fs::write(outside_path.join("b.toml"), "name = \"b\"\n").unwrap();
    breaks_after_a_pass(
        |p| std::os::unix::fs::symlink(&outside_path, p.join("mock/registry/linked")).unwrap(),
        |_| fs::write(outside_path.join("b.toml"), "bad = =\n").unwrap(),
    );
}

#[test]
fn real_taplo_a_hand_edited_generated_config_is_not_replayed() {
    breaks_after_a_pass(
        |p| {
            fs::write(p.join("custom.json"), "{}").unwrap();
            let cfg = fs::read_to_string(p.join(".taplo.toml")).unwrap();
            fs::write(p.join(".taplo.toml"), format!("{cfg}\n[[rule]]\ninclude = [\"mock/registry/**/*.toml\"]\nschema.path = \"custom.json\"\n")).unwrap();
        },
        |p| fs::write(p.join("custom.json"), "{\"type\":\"object\",\"required\":[\"need\"]}").unwrap(),
    );
}

#[test]
fn real_taplo_a_schema_bound_by_an_escaped_key_or_a_spaced_directive_is_not_replayed() {
    for (name, body) in [
        ("escaped", "\"\\U00000024schema\" = \"./s.json\"\nname = \"x\"\n"),
        ("spaced", "#: schema ./s.json\nname = \"x\"\n"),
        ("tabbed", "#:\tschema ./s.json\nname = \"x\"\n"),
    ] {
        eprintln!("case {name}");
        breaks_after_a_pass(
            |p| {
                fs::create_dir_all(p.join("other")).unwrap();
                fs::write(p.join("other/s.json"), "{}").unwrap();
                fs::write(p.join("other/x.toml"), body).unwrap();
            },
            |p| fs::write(p.join("other/s.json"), "{\"type\":\"object\",\"required\":[\"need\"]}").unwrap(),
        );
    }
}

#[test]
fn real_taplo_a_directory_reached_by_a_second_name_is_checked_under_both() {
    for dir in ["a1", "x", "q", "w", "b2", "c3"] {
        breaks_after_a_pass(
            |p| {
                fs::create_dir_all(p.join("mock").join(dir)).unwrap();
                fs::write(p.join("mock").join(dir).join("x.toml"), "name = \"x\"\n").unwrap();
                fs::write(
                    p.join("target/mockspace/registry-schemas/registry.schema.json"),
                    "{\"type\":\"object\",\"required\":[\"vocab\"]}",
                )
                .unwrap();
            },
            |p| std::os::unix::fs::symlink(Path::new("..").join(dir), p.join("mock/registry/linked")).unwrap(),
        );
    }
}

#[test]
fn a_file_edited_while_taplo_runs_does_not_get_the_pass_saved_under_its_old_content() {
    let f = Fixture::new();
    let reg = f.root.path().join("mock/registry/a.toml");
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'taplo 0.10.0'; exit 0; fi\n\
         echo run >> '{runs}'\necho 'found files total=2 excluded=0' >&2\n\
         if [ ! -f '{flag}' ]; then touch '{flag}'; echo '# edited during the run' >> '{reg}'; fi\nexit 0\n",
        runs = f.bin.path().join("runs").display(),
        flag = f.bin.path().join("flag").display(),
        reg = reg.display(),
    );
    let original = fs::read_to_string(&reg).unwrap();
    let path = f.taplo_path();
    let _ = fs::remove_file(&path);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(f.passes());
    fs::write(&reg, original).unwrap();
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "the first pass began on content that moved, so it was not saved");
}

#[test]
fn a_failed_version_probe_is_never_keyed() {
    let f = Fixture::new();
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'taplo 0.10.0'; exit 1; fi\n\
         echo run >> '{runs}'\necho 'found files total=2 excluded=0' >&2\nexit 0\n",
        runs = f.bin.path().join("runs").display(),
    );
    let path = f.taplo_path();
    let _ = fs::remove_file(&path);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(f.passes());
    assert!(f.passes());
    assert_eq!(f.runs(), 2, "a validator that cannot say its version is run every time");
}

#[test]
fn taplo_config_and_rust_log_in_the_environment_are_read_and_stop_the_replay() {
    for var in ["TAPLO_CONFIG", "RUST_LOG"] {
        let f = Fixture::new();
        let _serial = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: every check in this module holds `ENV_LOCK`, so no other thread reads the
        // environment while it is set.
        unsafe { std::env::set_var(var, "x") };
        drop(_serial);
        assert!(f.passes());
        assert!(f.passes());
        let _serial = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        unsafe { std::env::remove_var(var) };
        drop(_serial);
        assert_eq!(f.runs(), 2, "{var} set: taplo ran both times");
    }
}
