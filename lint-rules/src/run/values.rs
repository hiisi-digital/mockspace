//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the engine computed for a run, answered by name, so a trace that read a value is asked it
//! again on the next run.

use crate::context::{CrateInput, RepoInput, WorkspaceInput};
use crate::reading::Values;

/// The values of one run, under the names the contexts write them down by.
pub(crate) struct RunValues<'a> {
    pub(crate) ws:     &'a WorkspaceInput,
    pub(crate) crates: &'a [CrateInput],
    pub(crate) repo:   Option<RepoInput<'a>>,
    /// The reading's root, which a unit that was handed an absolute path is saved under.
    pub(crate) root:   &'a std::path::Path,
}

fn joined<S: AsRef<str>>(items: &[S]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in items {
        out.extend_from_slice(s.as_ref().as_bytes());
        out.push(0);
    }
    out
}

impl Values for RunValues<'_> {
    fn material(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(name) = key.strip_prefix("deps:") {
            return self.crates.iter().find(|c| c.name == name).map(|c| joined(&c.deps));
        }
        let ws = self.ws;
        Some(match key {
            "all_crates" => joined(&ws.all_crates.iter().collect::<Vec<_>>()),
            "workspace_root" => ws.workspace_root.to_string_lossy().into_owned().into_bytes(),
            "root" => self.root.to_string_lossy().into_owned().into_bytes(),
            "proc_macro_crates" => joined(&ws.proc_macro_crates),
            "primitive_introductions" => format!("{:?}", ws.primitive_introductions).into_bytes(),
            "invocation:unwanted" => Vec::new(),
            _ => {
                let repo = self.repo?;
                match key {
                    "repo_all_crates" => joined(&repo.all_crates.iter().collect::<Vec<_>>()),
                    "src_dirs" => format!("{:?}", repo.src_dirs).into_bytes(),
                    "invocation" => format!("{:?}", repo.invocation).into_bytes(),
                    "canon_paths" => joined(repo.canon_paths),
                    "open_panels" => joined(repo.open_panels),
                    "ref_files" => format!("{:?}", repo.ref_files).into_bytes(),
                    "registry" => format!("{:?}", repo.registry).into_bytes(),
                    _ => return None,
                }
            },
        })
    }
}
