//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The world the registry-on-a-ref tests share: a bare remote, clones of it that mount the
//! `ticket` directory on a ref or keep it in the tree, and the small helpers they read rows with.

use std::path::PathBuf;

use mockspace_store::{RefPair, RefStore, RetryPolicy};

use super::*;
use mockspace_store::test_support::Remote;

pub const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[[registry.namespace]]
key = "ticket"
title = "Tickets"
description = "Work to do."

[[registry.namespace.field]]
name = "title"
type = "string"
required = true
description = "What."

[[registry.namespace.field]]
name = "answers"
type = "string[]"
description = "What was said."
"#;

pub const STORE_DECL: &str = "\n[[registry.store]]\ndir = \"ticket\"\n";

/// A bare remote and any number of clones of it, each a project with the config above.
pub struct World {
    pub remote: Remote,
    pub tmp:    PathBuf,
}

pub const TICKET_REF: &str = "refs/heads/mock/registry/ticket";

impl World {
    pub fn new() -> Self {
        let remote = Remote::new();
        let tmp = remote.tmp.path().to_path_buf();
        Self {
            remote,
            tmp,
        }
    }

    /// The root of a clone whose project mounts `ticket` on the ref (`mounted`) or keeps it in
    /// the tree, its config written.
    pub fn project(&self, name: &str, mounted: bool) -> PathBuf {
        let root = self.remote.clone_dir(name);
        let config = format!("{CONFIG}{}", if mounted { STORE_DECL } else { "" });
        std::fs::write(root.join("mock/mockspace.toml"), config).unwrap();
        root
    }

    /// Such a clone, as the registry reads it, with its schemas generated.
    pub fn env_of(&self, name: &str, mounted: bool) -> Env {
        let root = self.project(name, mounted);
        let cfg = Env::from_dir(&root.join("mock")).unwrap();
        crate::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.namespaces);
        cfg
    }

    pub fn remote_ref(&self) -> Option<String> {
        self.remote.tip(TICKET_REF)
    }

    pub fn remote_file(&self, path: &str) -> Option<String> {
        self.remote.file(TICKET_REF, path)
    }
}
/// The ref-mounted stores of `cfg`, with no waiting between attempts.
pub fn stores(cfg: &dyn RegistryEnv) -> RegistryStores {
    let pair = RefPair::branch_form("registry/ticket").unwrap();
    let store = RefStore::new(cfg.repo_root(), "origin", pair).with_retry(RetryPolicy::immediate(4));
    RegistryStores::directory(cfg.mock_dir())
        .mount("ticket", Box::new(store))
        .unwrap()
}

pub fn row(id: &str, title: &str) -> String {
    format!("[[ticket]]\nid = \"{id}\"\ntitle = \"{title}\"\n\n")
}

pub fn put(rel: &str, text: &str) -> Vec<(String, Option<Vec<u8>>)> {
    vec![(rel.to_string(), Some(text.as_bytes().to_vec()))]
}

pub fn ids(reg: &Registry) -> Vec<String> {
    reg.rows.keys().cloned().collect()
}
