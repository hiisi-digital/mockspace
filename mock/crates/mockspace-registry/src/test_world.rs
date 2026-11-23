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

/// A namespace declaring `fields` as `(name, type, required)` and folding `path` into its ids.
pub fn namespace(key: &str, fields: &[(&str, &str, bool)], path: &[&str]) -> RegistryNamespace {
    RegistryNamespace {
        key:         key.into(),
        title:       None,
        description: None,
        value_field: None,
        group_by:    None,
        path:        path.iter().map(|p| (*p).into()).collect(),
        fields:      fields
            .iter()
            .map(|(name, ty, required)| {
                RegistryField {
                    name:        (*name).into(),
                    r#type:      (*ty).into(),
                    required:    *required,
                    description: None,
                    visibility:  FieldVisibility::Public,
                    values:      Vec::new(),
                    min:         None,
                    max:         None,
                }
            })
            .collect(),
    }
}

/// The registry the loader makes of one file `t.toml` holding `text`.
pub fn rows_from(text: &str, namespaces: &[RegistryNamespace]) -> Registry {
    crate::load::rows_of_texts(Path::new("reg"), vec![("t.toml".to_string(), Some(text.to_string()))], namespaces, None)
}

/// The registry the loader makes of several files, each `(name, text)`.
pub fn rows_from_files(files: &[(&str, &str)], namespaces: &[RegistryNamespace]) -> Registry {
    let files = files.iter().map(|(n, t)| ((*n).to_string(), Some((*t).to_string()))).collect();
    crate::load::rows_of_texts(Path::new("reg"), files, namespaces, None)
}
