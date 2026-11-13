//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The reading, the view and the store, held to what the topic says of them.
//!
//! Every test here runs on real files in a scratch directory, through [`crate::run`] where a store
//! is involved, so what is measured is what a run does and not what one function does alone.

mod each;
mod history;
mod invalidation;
mod keys;
mod moved;
mod order;
mod overlay;
mod sharing;
mod spread;

use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::context::{CrateInput, RepoInput, WorkspaceInput};
use crate::reading::Reading;
use crate::verdict::VerdictCache;
use crate::{Lint, LintConfig, LintError, LintPack, RepoContext, RepoLint, RunInput, Severity, View};

/// A store held in memory, counting what was written to it.
#[derive(Default)]
pub(super) struct Mem {
    entries:    Mutex<HashMap<String, String>>,
    remembered: AtomicUsize,
}

impl Mem {
    pub(super) fn remembered(&self) -> usize {
        self.remembered.load(Ordering::SeqCst)
    }
}

impl VerdictCache for Mem {
    fn digest(&self, parts: &[&[u8]]) -> String {
        let mut out = String::with_capacity(32);
        for salt in [0u8, 1u8] {
            let mut h = std::hash::DefaultHasher::new();
            salt.hash(&mut h);
            for p in parts {
                p.len().hash(&mut h);
                p.hash(&mut h);
            }
            out.push_str(&format!("{:016x}", h.finish()));
        }
        out
    }

    fn replay(&self, key: &str) -> Option<String> {
        self.entries.lock().unwrap().get(key).cloned()
    }

    fn remember(&self, key: &str, encoded: &str) {
        self.remembered.fetch_add(1, Ordering::SeqCst);
        self.entries.lock().unwrap().insert(key.to_string(), encoded.to_string());
    }
}

/// A scratch directory, removed on drop.
pub(super) struct Dir(pub(super) PathBuf);

impl Dir {
    pub(super) fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("mockspace-reading-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    pub(super) fn write(&self, rel: &str, text: &str) -> &Self {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
        self
    }

    pub(super) fn remove(&self, rel: &str) {
        std::fs::remove_file(self.0.join(rel)).unwrap();
    }

    /// Run `git` here, panicking where it fails.
    pub(super) fn git(&self, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A repo lint asking its view one thing, counting the times it was judged, and reporting the
/// answer as its one finding.
pub(super) struct Probe {
    pub(super) judged: Arc<AtomicUsize>,
    pub(super) ask:    fn(&View<'_>) -> String,
}

impl Probe {
    pub(super) fn new(ask: fn(&View<'_>) -> String) -> (Self, Arc<AtomicUsize>) {
        let judged = Arc::new(AtomicUsize::new(0));
        (
            Self {
                judged: Arc::clone(&judged),
                ask,
            },
            judged,
        )
    }
}

impl Lint for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for Probe {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        self.judged.fetch_add(1, Ordering::SeqCst);
        vec![LintError::error("workspace".into(), 1, "probe", (self.ask)(ctx.view()))]
    }
}

/// What a run gave back, and what its reading did to get it.
pub(super) struct Ran {
    pub(super) found:  Vec<LintError>,
    pub(super) counts: crate::reading::Counts,
}

/// One run of the repo lints of `pack` alone over `dir`, a fresh reading as every run has, saving
/// to `cache`.
pub(super) fn run_repo(dir: &Dir, cache: &Mem, pack: &LintPack, overrides: Option<&LintConfig>) -> Ran {
    run_repo_over(Reading::new(&dir.0), dir, cache, pack, overrides)
}

pub(super) fn run_repo_over(reading: Reading, dir: &Dir, cache: &Mem, pack: &LintPack, overrides: Option<&LintConfig>) -> Ran {
    let none = BTreeSet::new();
    let repo = RepoInput {
        mock_dir:    &dir.0,
        repo_root:   &dir.0,
        all_crates:  &none,
        src_dirs:    &[],
        invocation:  None,
        canon_paths: &[],
        open_panels: &[],
        ref_files:   &crate::src_layout::NO_REF_FILES,
        registry:    &Default::default(),
    };
    let ws = WorkspaceInput::default();
    let only = pack.repo_lints.first().map(|l| l.name());
    let input = RunInput {
        reading: &reading,
        cache: Some(cache as &dyn VerdictCache),
        overrides,
        doc_only: false,
        workspace: &ws,
        crates: &[],
        scope: None,
        repo: Some(repo),
        only,
    };
    let found = crate::run(&input, Some(pack));
    Ran {
        found,
        counts: reading.counts(),
    }
}

pub(super) fn pack_of(lint: impl RepoLint + 'static) -> LintPack {
    LintPack {
        repo_lints: vec![Box::new(lint)],
        ..LintPack::default()
    }
}

pub(super) fn messages(found: &[LintError]) -> Vec<String> {
    found.iter().map(|e| e.message.clone()).collect()
}

/// A workspace on disk: crates under `crates/`, each a `src/` of files.
pub(super) struct Workspace {
    pub(super) dir:    Dir,
    pub(super) crates: Vec<CrateInput>,
    pub(super) ws:     WorkspaceInput,
}

impl Workspace {
    pub(super) fn new(name: &str, crates: &[(&str, &[(&str, &str)])]) -> Self {
        let dir = Dir::new(name);
        let mut inputs = Vec::new();
        let mut all = BTreeSet::new();
        for (krate, files) in crates {
            for (rel, text) in *files {
                dir.write(&format!("crates/{krate}/{rel}"), text);
            }
            all.insert((*krate).to_string());
            inputs.push(CrateInput {
                name:       (*krate).to_string(),
                short_name: (*krate).to_string(),
                deps:       Vec::new(),
                dir:        dir.0.join("crates").join(krate),
            });
        }
        let ws = WorkspaceInput {
            all_crates: all,
            workspace_root: dir.0.clone(),
            crate_prefix: String::new(),
            ..WorkspaceInput::default()
        };
        Self {
            dir,
            crates: inputs,
            ws,
        }
    }

    pub(super) fn write(&self, krate: &str, rel: &str, text: &str) {
        self.dir.write(&format!("crates/{krate}/{rel}"), text);
    }

    pub(super) fn remove(&self, krate: &str, rel: &str) {
        self.dir.remove(&format!("crates/{krate}/{rel}"));
    }

    /// Every lint, built-in and from `pack`, over the workspace and the repository, with the
    /// given reading, cache and configuration.
    pub(super) fn run_with(
        &self,
        reading: &Reading,
        cache: Option<&Mem>,
        overrides: Option<&LintConfig>,
        pack: Option<&LintPack>,
    ) -> Vec<LintError> {
        let none = BTreeSet::new();
        let repo = RepoInput {
            mock_dir:    &self.dir.0,
            repo_root:   &self.dir.0,
            all_crates:  &none,
            src_dirs:    &[],
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            ref_files:   &crate::src_layout::NO_REF_FILES,
            registry:    &Default::default(),
        };
        let input = RunInput {
            reading,
            cache: cache.map(|c| c as &dyn VerdictCache),
            overrides,
            doc_only: false,
            workspace: &self.ws,
            crates: &self.crates,
            scope: None,
            repo: Some(repo),
            only: None,
        };
        crate::run(&input, pack)
    }

    pub(super) fn run(&self, cache: Option<&Mem>, overrides: Option<&LintConfig>, pack: Option<&LintPack>) -> Vec<LintError> {
        self.run_with(&Reading::new(&self.dir.0), cache, overrides, pack)
    }
}

/// Everything a finding says, as text, so two runs compare byte for byte.
pub(super) fn shown(found: &[LintError]) -> String {
    found.iter().map(|e| format!("{e:?}\n")).collect()
}

/// Set a file's modification time, the way a checkout or a restore from backup moves it.
pub(super) fn touch(path: &Path, offset_secs: u64) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(offset_secs);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}
