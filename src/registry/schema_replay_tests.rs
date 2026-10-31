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
const GENERATED_CONFIG: &str = "# AUTO-GENERATED: DO NOT EDIT DIRECTLY\ninclude = [\"mock/registry/**/*.toml\"]\n";

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
