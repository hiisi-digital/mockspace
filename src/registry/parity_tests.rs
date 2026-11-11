//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The engine and a tool without it read the same registry of the same tree.
//!
//! `Config` and `mockspace_registry::Env` both answer `RegistryEnv`. The engine's config hands the
//! declaration to the crate and takes what it derives back, and both find the project with the
//! crate's `locate`; these tests hold the two answers together over the layouts a project comes in.

use std::path::{Path, PathBuf};

use super::{Env, RegistryEnv};
use crate::config::Config;

const DECLARED: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[ref.roots.seed]
path = "seed"
frozen = true

[ref.roots.notes]
path = "mock/notes"
links = false
label = "the notes"
internal = true

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work to do."

[[registry.namespace.field]]
name = "title"
type = "string"
required = true
description = "What."

[[registry.store]]
dir = "ticket"
"#;

/// Everything a registry reads of its project, as one text, so two environments are compared whole.
fn seen(env: &dyn RegistryEnv) -> String {
    format!(
        "root {:?}\nmock {:?}\ndocs {:?}\nnamespaces {:?}\nstores {:?}\nroots {:?}\nfrozen {:?}\nprose {:?}\ninternal {:?}",
        env.repo_root(),
        env.mock_dir(),
        env.docs_dir(),
        env.namespaces(),
        env.stores(),
        env.roots(),
        env.frozen_roots(),
        env.prose_roots(),
        env.internal_roots(),
    )
}

/// Where the config of a layout goes, if anywhere.
#[derive(Clone, Copy, Debug)]
enum At {
    Mock,
    Root,
    Both,
    Nowhere,
}

/// A project under a fresh directory: `mock/` below a root that holds `.git` or does not, with the
/// config where `at` says. Gives the directory and the mock directory.
fn layout(git: bool, at: At) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("project");
    let mock = root.join("mock");
    std::fs::create_dir_all(&mock).unwrap();
    if git {
        mockspace_store::test_support::git(&root, &["init", "-q"]);
    }
    let write = |dir: &Path, text: &str| std::fs::write(dir.join("mockspace.toml"), text).unwrap();
    match at {
        At::Mock => write(&mock, DECLARED),
        At::Root => write(&root, DECLARED),
        // the one in the mock directory is the one read, so the root's declares something else
        At::Both => {
            write(&mock, DECLARED);
            write(&root, "project_name = \"other\"\n");
        },
        At::Nowhere => {},
    }
    (tmp, mock)
}

#[test]
fn the_engine_and_a_tool_without_it_derive_the_same_registry_in_every_layout() {
    for git in [true, false] {
        for at in [At::Mock, At::Root, At::Both, At::Nowhere] {
            let (_tmp, mock) = layout(git, at);
            let engine = Config::from_dir(&mock);
            let alone = Env::from_dir(&mock).unwrap();
            assert_eq!(seen(&engine), seen(&alone), "git {git}, config {at:?}");
            let declares = seen(&alone).contains("\"ticket\"");
            assert_eq!(declares, !matches!(at, At::Nowhere), "git {git}, config {at:?}: the declaration was read");
        }
    }
}

#[test]
fn the_comparison_sees_a_registry_that_differs() {
    // the control of the test above: two projects that declare different things do not read alike
    let (_a, with) = layout(true, At::Mock);
    let (_b, without) = layout(true, At::Nowhere);
    let (with, without) = (Env::from_dir(&with).unwrap(), Env::from_dir(&without).unwrap());
    let masked = |env: &Env| seen(env).lines().skip(3).collect::<Vec<_>>().join("\n");
    assert_ne!(masked(&with), masked(&without));
}

#[test]
fn the_kinds_the_engine_emits_itself_are_listed_once_and_emitted() {
    let dispatch = include_str!("../entry/dispatch.rs");
    for kind in super::RENDER_FINDING_KINDS {
        assert!(dispatch.contains(&format!("kind: \"{kind}\"")), "`{kind}` is listed and the engine does not emit it");
        assert!(!super::FINDING_KINDS.contains(kind), "`{kind}` is in the registry crate's list too");
    }
}
