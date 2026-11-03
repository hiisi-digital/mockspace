//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The world the registry-on-a-ref tests share: a bare remote, clones of it that mount the
//! `ticket` directory on a ref or keep it in the tree, and the small helpers they read rows with.

use std::path::PathBuf;

use mockspace_store::{RefPair, RefStore, RetryPolicy};

use super::*;
use crate::config::Config;
use crate::test_remote::Remote;

pub(crate) const CONFIG: &str = r#"project_name = "fixture"
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

pub(crate) const STORE_DECL: &str = "\n[[registry.store]]\ndir = \"ticket\"\n";

/// A bare remote and any number of clones of it, each a project with the config above.
pub(crate) struct World {
    pub(crate) remote: Remote,
    pub(crate) tmp:    PathBuf,
}

pub(crate) const TICKET_REF: &str = "refs/heads/mock/registry/ticket";

impl World {
    pub(crate) fn new() -> Self {
        let remote = Remote::new();
        let tmp = remote.tmp.path().to_path_buf();
        Self {
            remote,
            tmp,
        }
    }

    /// A clone whose project mounts `ticket` on the ref (`mounted`) or keeps it in the tree.
    pub(crate) fn clone_of(&self, name: &str, mounted: bool) -> Config {
        let root = self.remote.clone_dir(name);
        let config = format!("{CONFIG}{}", if mounted { STORE_DECL } else { "" });
        std::fs::write(root.join("mock/mockspace.toml"), config).unwrap();
        let cfg = Config::from_dir(&root.join("mock"));
        crate::registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
        cfg
    }

    pub(crate) fn remote_ref(&self) -> Option<String> {
        self.remote.tip(TICKET_REF)
    }

    pub(crate) fn remote_file(&self, path: &str) -> Option<String> {
        self.remote.file(TICKET_REF, path)
    }
}
/// The ref-mounted stores of `cfg`, with no waiting between attempts.
pub(crate) fn stores(cfg: &Config) -> RegistryStores {
    let pair = RefPair::branch_form("registry/ticket").unwrap();
    let store = RefStore::new(&cfg.repo_root, "origin", pair).with_retry(RetryPolicy::immediate(4));
    RegistryStores::directory(&cfg.mock_dir)
        .mount("ticket", Box::new(store))
        .unwrap()
}

pub(crate) fn row(id: &str, title: &str) -> String {
    format!("[[ticket]]\nid = \"{id}\"\ntitle = \"{title}\"\n\n")
}

pub(crate) fn put(rel: &str, text: &str) -> Vec<(String, Option<Vec<u8>>)> {
    vec![(rel.to_string(), Some(text.as_bytes().to_vec()))]
}

pub(crate) fn ids(reg: &Registry) -> Vec<String> {
    reg.rows.keys().cloned().collect()
}
