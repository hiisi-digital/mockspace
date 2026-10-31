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
        fs::write(f.root.path().join(".taplo.toml"), "include = [\"mock/registry/**/*.toml\"]\n").unwrap();
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
    fs::write(f.root.path().join(".taplo.toml"), "include = [\"mock/**/*.toml\"]\n").unwrap();
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
