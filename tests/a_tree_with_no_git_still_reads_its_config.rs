//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A tree that is not a git repository still reads the config at its root.
//!
//! ## What went wrong
//!
//! `Config::from_dir` takes the mock dir and finds the repo root by walking up
//! for a `.git`. The config sits at that root once relocated, so a tree with no
//! `.git` finds no root, falls back to the mock dir, looks for the config
//! inside it, does not find it there either, and reads the empty string. Every
//! key then takes its default: no registry namespaces, no declared roots, no
//! lint configuration. Nothing is printed and nothing fails.
//!
//! What that produces is a run that reports success over a config it never
//! opened. Measured on one repo with 46 rows in one declared namespace: a
//! clone answers `46 rows across 1 namespaces`, and a copy of the same tree
//! with `.git` excluded answers `0 rows across 0 namespaces` and names its
//! namespaces as `vocab, reference`, which are the two the engine supplies
//! itself. Every registry lint over that tree passed by having nothing to read.
//!
//! ## Why a tree with no `.git` is ordinary rather than exotic
//!
//! An export, a tarball, a vendored copy, a rsync that excluded the admin
//! directory, a build container given the sources and not the history. None of
//! those is a mistake, and the tool's answer over them has to be either right
//! or loud.

use std::fs;
use std::path::Path;

use mockspace::config::Config;

/// The config a repo carries at its root once relocated: the mock dir named,
/// and one registry namespace that nothing else supplies.
const CONFIG: &str = r#"
project_name = "probe"
mock_dir = "mock"

[[registry.namespace]]
key = "question"
title = "Question"

[[registry.namespace.field]]
name = "text"
type = "string"
"#;

/// A tree shaped like a repo whose config sits at the root, with `.git`
/// present or absent. Returns the mock dir, which is what the launcher hands
/// `Config::from_dir`.
fn tree(git: bool) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::write(root.join("mockspace.toml"), CONFIG).expect("write config");
    let mock = root.join("mock");
    fs::create_dir_all(mock.join("registry")).expect("create mock dir");
    if git {
        fs::create_dir_all(root.join(".git")).expect("create .git");
    }
    (tmp, mock)
}

/// A namespace declared in the config, read back off a loaded `Config`. The
/// builtins are filtered out because the engine supplies those whether the
/// config was opened or not, so their presence says nothing.
fn declared_namespaces(cfg: &Config) -> Vec<String> {
    cfg.registry_namespaces
        .iter()
        .map(|n| n.key.clone())
        .filter(|k| k != "vocab" && k != "reference")
        .collect()
}

#[test]
fn a_tree_with_no_git_reads_the_config_at_its_root() {
    let (_tmp, mock) = tree(false);
    let cfg = Config::from_dir(&mock);
    assert_eq!(
        declared_namespaces(&cfg),
        vec!["question".to_string()],
        "the config is at the root of a tree with no `.git`, and it is the same \
         file the same tree hands over once `.git` is there: {}",
        cfg.config_path.display()
    );
}

/// The positive control. Without it the arm above passes against an
/// implementation that declares `question` unconditionally, and it is also
/// what says the fixture is shaped like a config the loader understands at
/// all: if this one fails, the arm above proves nothing about `.git`.
#[test]
fn the_same_tree_with_a_git_dir_reads_it_too() {
    let (_tmp, mock) = tree(true);
    let cfg = Config::from_dir(&mock);
    assert_eq!(
        declared_namespaces(&cfg),
        vec!["question".to_string()],
        "the fixture is a config the loader reads when the root is findable"
    );
}

/// The other half of the control: a tree carrying no config at all still ends
/// with no declared namespaces, so the arms above are reading a file rather
/// than reporting whatever the loader defaults to.
#[test]
fn a_tree_with_no_config_declares_no_namespaces() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mock = tmp.path().join("mock");
    fs::create_dir_all(&mock).expect("create mock dir");
    let cfg = Config::from_dir(&mock);
    assert!(
        declared_namespaces(&cfg).is_empty(),
        "nothing declared them: {:?}",
        declared_namespaces(&cfg)
    );
}

/// The other way a config goes unread, on the same line and with the same
/// silence: present, malformed, and discarded whole. Every key the author
/// wrote then governs nothing.
///
/// Found by this file's own fixture. The first draft declared a registry field
/// as `key` where the type wants `name`, and the load answered with the empty
/// configuration rather than with a complaint, which is why both arms above
/// failed identically and read as one defect instead of two.
#[test]
#[should_panic(expected = "does not parse")]
fn a_config_that_does_not_parse_is_refused_rather_than_defaulted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Valid TOML, and not a config: the field key is wrong, which is the
    // ordinary way one of these is malformed.
    fs::write(
        root.join("mockspace.toml"),
        "[[registry.namespace]]\nkey = \"question\"\n\n[[registry.namespace.field]]\nkey = \
         \"text\"\n",
    )
    .expect("write config");
    let mock = root.join("mock");
    fs::create_dir_all(&mock).expect("create mock dir");
    let _ = Config::from_dir(&mock);
}

/// The control on that refusal: it fires on a config that is there and wrong,
/// never on a project that keeps none. Without this arm an implementation that
/// panics on every load satisfies the one above.
#[test]
fn a_project_with_no_config_at_all_still_loads() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mock = tmp.path().join("mock");
    fs::create_dir_all(&mock).expect("create mock dir");
    let cfg = Config::from_dir(&mock);
    assert_eq!(
        cfg.project_name, "project",
        "the default name, reached without a word said"
    );
}

/// A config that was never opened is the failure this file is about, so the
/// path it settled on is worth asserting directly: it names a file that
/// exists. `from_dir` records the path it read from, and a run over a tree
/// with no `.git` used to record one that is not there.
#[test]
fn the_path_a_load_records_is_a_file_that_exists() {
    let (_tmp, mock) = tree(false);
    let cfg = Config::from_dir(&mock);
    assert!(
        Path::new(&cfg.config_path).is_file(),
        "the recorded config path has to be openable, or the load read nothing: {}",
        cfg.config_path.display()
    );
}
