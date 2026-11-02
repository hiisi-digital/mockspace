//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A bare git repository and clones of it, for the tests of the stores kept on a ref: real
//! git, real pushes, nothing mocked.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run git in `dir`, panic with its words when it fails, give its output trimmed.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A bare remote, and the directory its clones are made in.
pub(crate) struct Remote {
    pub(crate) tmp:  tempfile::TempDir,
    pub(crate) bare: PathBuf,
}

impl Remote {
    pub(crate) fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join("remote.git");
        std::fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        Self { tmp, bare }
    }

    /// A repository `name` whose `origin` is the bare one, with a `mock/` directory in it.
    pub(crate) fn clone_dir(&self, name: &str) -> PathBuf {
        let root = self.tmp.path().join(name);
        std::fs::create_dir_all(root.join("mock")).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["remote", "add", "origin", self.bare.to_str().unwrap()]);
        root
    }

    /// The commit at `reference` on the remote, if there is one.
    pub(crate) fn tip(&self, reference: &str) -> Option<String> {
        let out = git(&self.bare, &["for-each-ref", "--format=%(objectname)", reference]);
        (!out.is_empty()).then_some(out)
    }

    /// The text of `path` at `reference` on the remote.
    pub(crate) fn file(&self, reference: &str, path: &str) -> Option<String> {
        Command::new("git")
            .arg("-C")
            .arg(&self.bare)
            .args(["show", &format!("{reference}:{path}")])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    }
}
