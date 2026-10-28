//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a test's generated lint crate goes: inside the test's own directory.
//!
//! The engine builds a repo's lints into a crate in a shared store, under the
//! user's cache unless `CARGO_TARGET_DIR` says otherwise, keyed by the repo's
//! identity. A fixture repository has no remote, so every fixture is a key of
//! its own, and a test that did not point the store elsewhere wrote about 85 MB
//! into the developer's cache per run, which nothing deletes. Seventeen of them
//! were 792 MB.
//!
//! So every test that builds a lint crate goes through this file, either by
//! loading with [`load`] or by running the engine with [`isolate`], and the
//! store is a directory beside the fixture that goes when the fixture does.
//! Included by path, like `ambient.rs`.

#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The store for a test whose scratch directory is `tmp`.
pub fn cache_in(tmp: &Path) -> PathBuf {
    tmp.join("lint-cache")
}

/// The store for a fixture whose mock directory is `mock`: beside it.
pub fn cache_beside(mock: &Path) -> PathBuf {
    cache_in(mock.parent().unwrap_or(mock))
}

/// `mockspace::custom_lints::load` with the store beside `mock`.
pub fn load(
    mock: &Path,
    cfg: &mockspace::config::Config,
    config_path: &Path,
    lint_rules_dep: &str,
) -> Result<Option<mockspace::custom_lints::LoadedLints>, String> {
    mockspace::custom_lints::load_in(&cache_beside(mock), cfg, config_path, lint_rules_dep)
}

/// Run the engine with its store beside `mock` and no shared target directory
/// inherited, either of which would put the build somewhere the test does not
/// delete. A test that is about a target directory sets it after this.
pub fn isolate<'a>(cmd: &'a mut Command, mock: &Path) -> &'a mut Command {
    cmd.env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env("XDG_CACHE_HOME", cache_beside(mock))
}
