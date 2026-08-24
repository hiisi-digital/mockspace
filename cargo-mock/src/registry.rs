//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The global launcher registry: a machine-wide record of which repos consume
//! which built engine, used to garbage-collect engine builds nothing pins
//! anymore and, forward-looking, to index every repo the launcher has seen.
//!
//! It lives at `~/.cache/mockspace/registry.toml`, beside `builds/`. Everything
//! in it is recomputable: re-read each repo's `mockspace.toml`, re-resolve, and
//! the same rows reappear. That is why it is cache, not config, a wipe costs
//! only the first re-run of each repo. The durable per-developer state the v2
//! spec places under `~/.config/mockspace/` (the TOFU `trust.toml`) is a
//! separate concern this file does not touch.
//!
//! Two tables:
//!
//! - `[[consumer]]`, one per repo the launcher has run: where it is, what it
//!   pins, and which build key that resolved to. The pin `form` (`version` /
//!   `branch` / `rev` / `tag` / `legacy`) makes migration detection fall out of
//!   the registry: a `legacy` consumer has not adopted an explicit pin yet.
//! - `[[build]]`, one per cached engine build: its key and when it was last
//!   used. GC removes a build no live consumer resolves to.
//!
//! The `name` / `mock_dir` / `engine_url` fields are kept so a later cross-repo
//! index (the v2 `[hosts.*]` alias direction) has its data already; the launcher
//! does not resolve cross-repo references itself yet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// GC runs at most this often; a `last_gc` within the window skips the pass.
const GC_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// A build whose consumers have all been idle at least this long is evicted
/// even though they still nominally pin it (the LRU overlay op asked for): an
/// untouched repo's engine is cheap to rebuild if the repo is ever revisited.
const LRU_STALE_SECS: u64 = 30 * 24 * 60 * 60;

/// Pin form as recorded for a consumer. `Legacy` means the pin came from the
/// mock workspace's `Cargo.lock` rather than an explicit `mockspace_*` key, so
/// the repo has not been migrated to the launcher+pin model yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinForm {
    Version,
    Branch,
    Rev,
    Tag,
    Legacy,
}

impl PinForm {
    fn as_str(self) -> &'static str {
        match self {
            PinForm::Version => "version",
            PinForm::Branch => "branch",
            PinForm::Rev => "rev",
            PinForm::Tag => "tag",
            PinForm::Legacy => "legacy",
        }
    }
}

/// The whole registry file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    /// Schema tag for forward migrations; absent on the first write.
    #[serde(default)]
    pub schema:    u32,
    /// Unix seconds of the last GC pass, throttling the next one.
    #[serde(default)]
    pub last_gc:   u64,
    #[serde(default, rename = "consumer")]
    pub consumers: Vec<Consumer>,
    #[serde(default, rename = "build")]
    pub builds:    Vec<Build>,
}

/// One repo the launcher has run in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consumer {
    /// Absolute repo root.
    pub root:       String,
    /// Repo name (the root dir basename today; a real project name later).
    pub name:       String,
    /// Absolute mock dir the pin maps.
    pub mock_dir:   String,
    /// The engine source url this repo builds from.
    pub engine_url: String,
    /// `version` / `branch` / `rev` / `tag` / `legacy`.
    pub pin_form:   String,
    /// The pin value (version string, branch name, rev, tag), empty for legacy.
    #[serde(default)]
    pub pin_value:  String,
    /// The build key this consumer last resolved to.
    pub key:        String,
    /// Unix seconds of the last run in this repo.
    pub last_seen:  u64,
}

/// One cached engine build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub key:        String,
    pub engine_url: String,
    pub key_rev:    String,
    #[serde(default)]
    pub toolchain:  String,
    pub built_at:   u64,
    pub last_used:  u64,
}

/// The registry file path under the cache root.
pub fn registry_path(cache_root: &Path) -> PathBuf {
    cache_root.join("registry.toml")
}

impl Registry {
    /// Load the registry, tolerating absence or corruption by returning an empty
    /// one (it is a rebuildable cache, never a hard dependency).
    pub fn load(path: &Path) -> Registry {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Serialize and write. Best-effort: a write failure is not worth failing a
    /// `mock` invocation over, since the registry is only an optimisation. The
    /// write is atomic (temp beside the target, then rename) so a concurrent
    /// launcher reading the registry never sees a half-written file.
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let tmp = path.with_extension("toml.tmp");
            if std::fs::write(&tmp, &text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// Upsert the current repo as a consumer (keyed by `root`) and the build it
    /// resolved to (keyed by `key`), stamping `now`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        root: &str,
        name: &str,
        mock_dir: &str,
        engine_url: &str,
        form: PinForm,
        pin_value: &str,
        key: &str,
        key_rev: &str,
        toolchain: &str,
        now: u64,
    ) {
        match self.consumers.iter_mut().find(|c| c.root == root) {
            Some(c) => {
                c.name = name.to_string();
                c.mock_dir = mock_dir.to_string();
                c.engine_url = engine_url.to_string();
                c.pin_form = form.as_str().to_string();
                c.pin_value = pin_value.to_string();
                c.key = key.to_string();
                c.last_seen = now;
            },
            None => {
                self.consumers.push(Consumer {
                    root:       root.to_string(),
                    name:       name.to_string(),
                    mock_dir:   mock_dir.to_string(),
                    engine_url: engine_url.to_string(),
                    pin_form:   form.as_str().to_string(),
                    pin_value:  pin_value.to_string(),
                    key:        key.to_string(),
                    last_seen:  now,
                })
            },
        }
        match self.builds.iter_mut().find(|b| b.key == key) {
            Some(b) => b.last_used = now,
            None => {
                self.builds.push(Build {
                    key:        key.to_string(),
                    engine_url: engine_url.to_string(),
                    key_rev:    key_rev.to_string(),
                    toolchain:  toolchain.to_string(),
                    built_at:   now,
                    last_used:  now,
                })
            },
        }
    }

    /// Whether a GC pass is due (throttled by `last_gc`).
    pub fn gc_due(&self, now: u64) -> bool {
        now.saturating_sub(self.last_gc) >= GC_INTERVAL_SECS
    }

    /// Garbage-collect the build cache. A build is removed when no live consumer
    /// resolves to its key: either no consumer pins it at all (orphan, e.g. a
    /// repo re-pinned to a newer engine), or every consumer that pins it has
    /// been idle past the LRU window. The `protect` key (the build resolving
    /// this very invocation) is never removed. Consumers whose repo root no
    /// longer exists on disk are dropped first, so a deleted repo orphans its
    /// build. Removes the on-disk build dirs and the pruned `[[build]]` rows;
    /// returns the removed keys for logging.
    pub fn gc(&mut self, cache_root: &Path, protect: &str, now: u64) -> Vec<String> {
        self.consumers.retain(|c| Path::new(&c.root).is_dir());
        self.last_gc = now;

        let builds_dir = cache_root.join("builds");
        let mut removed = Vec::new();
        let consumers = self.consumers.clone();
        self.builds.retain(|b| {
            if b.key == protect {
                return true;
            }
            let pinners: Vec<&Consumer> = consumers.iter().filter(|c| c.key == b.key).collect();
            let live = pinners
                .iter()
                .any(|c| now.saturating_sub(c.last_seen) < LRU_STALE_SECS);
            if live {
                return true;
            }
            // orphan (no pinners) or all pinners stale: evict.
            let _ = std::fs::remove_dir_all(builds_dir.join(&b.key));
            removed.push(b.key.clone());
            false
        });
        removed
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn touch_build_dir(cache_root: &Path, key: &str) {
        let d = cache_root.join("builds").join(key).join("bin");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("mockspace"), b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn record_upserts_consumer_and_build() {
        let mut r = Registry::default();
        r.record(
            "/r",
            "r",
            "/r/mock",
            "u",
            PinForm::Branch,
            "dev",
            "k1",
            "rev1",
            "tc",
            100,
        );
        r.record(
            "/r",
            "r",
            "/r/mock",
            "u",
            PinForm::Branch,
            "dev",
            "k1",
            "rev1",
            "tc",
            200,
        );
        assert_eq!(r.consumers.len(), 1);
        assert_eq!(r.consumers[0].last_seen, 200);
        assert_eq!(r.builds.len(), 1);
        assert_eq!(r.builds[0].last_used, 200);
        assert_eq!(r.consumers[0].pin_form, "branch");
    }

    #[test]
    fn record_repin_leaves_old_build_orphaned() {
        let mut r = Registry::default();
        r.record(
            "/r",
            "r",
            "/r/mock",
            "u",
            PinForm::Branch,
            "dev",
            "old",
            "r1",
            "tc",
            100,
        );
        // same repo re-pins to a new key: consumer moves, old build stays.
        r.record(
            "/r",
            "r",
            "/r/mock",
            "u",
            PinForm::Version,
            "0.0.1",
            "new",
            "r2",
            "tc",
            200,
        );
        assert_eq!(r.consumers.len(), 1);
        assert_eq!(r.consumers[0].key, "new");
        assert_eq!(r.builds.len(), 2); // old is now orphaned, GC removes it
    }

    #[test]
    fn roundtrips_through_toml() {
        let mut r = Registry::default();
        r.record(
            "/r",
            "r",
            "/r/mock",
            "u",
            PinForm::Rev,
            "abc",
            "k",
            "abc",
            "tc",
            100,
        );
        let dir = tempfile::tempdir().unwrap();
        let path = registry_path(dir.path());
        r.save(&path);
        let back = Registry::load(&path);
        assert_eq!(back.consumers.len(), 1);
        assert_eq!(back.builds.len(), 1);
        assert_eq!(back.consumers[0].root, "/r");
    }

    #[test]
    fn load_missing_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let r = Registry::load(&registry_path(dir.path()));
        assert!(r.consumers.is_empty() && r.builds.is_empty());
    }

    #[test]
    fn gc_removes_orphan_build_but_keeps_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // a real repo dir so its consumer is not dropped.
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        touch_build_dir(root, "pinned");
        touch_build_dir(root, "orphan");

        let mut r = Registry::default();
        r.record(
            repo.to_str().unwrap(),
            "repo",
            "/m",
            "u",
            PinForm::Branch,
            "dev",
            "pinned",
            "r",
            "tc",
            1000,
        );
        // an orphan build with no consumer at all.
        r.builds.push(Build {
            key:        "orphan".into(),
            engine_url: "u".into(),
            key_rev:    "r".into(),
            toolchain:  "tc".into(),
            built_at:   1,
            last_used:  1,
        });

        let removed = r.gc(root, "pinned", 2000);
        assert_eq!(removed, vec!["orphan".to_string()]);
        assert!(root.join("builds").join("pinned").is_dir());
        assert!(!root.join("builds").join("orphan").exists());
    }

    #[test]
    fn gc_evicts_build_whose_consumers_are_all_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        touch_build_dir(root, "stalekey");

        let mut r = Registry::default();
        // last_seen far in the past relative to `now`.
        r.record(
            repo.to_str().unwrap(),
            "repo",
            "/m",
            "u",
            PinForm::Branch,
            "dev",
            "stalekey",
            "r",
            "tc",
            1,
        );
        let now = LRU_STALE_SECS + 1000;
        let removed = r.gc(root, "somethingelse", now);
        assert_eq!(removed, vec!["stalekey".to_string()]);
    }

    #[test]
    fn gc_protects_current_key_even_if_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        touch_build_dir(root, "current");
        let mut r = Registry::default();
        r.record(
            repo.to_str().unwrap(),
            "repo",
            "/m",
            "u",
            PinForm::Branch,
            "dev",
            "current",
            "r",
            "tc",
            1,
        );
        let now = LRU_STALE_SECS + 1000;
        let removed = r.gc(root, "current", now);
        assert!(removed.is_empty());
        assert!(root.join("builds").join("current").is_dir());
    }

    #[test]
    fn gc_drops_consumer_whose_root_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch_build_dir(root, "k");
        let mut r = Registry::default();
        // consumer root does not exist on disk.
        r.record(
            "/no/such/repo",
            "gone",
            "/m",
            "u",
            PinForm::Branch,
            "dev",
            "k",
            "r",
            "tc",
            1000,
        );
        let removed = r.gc(root, "protect-nothing", 2000);
        assert!(r.consumers.is_empty());
        assert_eq!(removed, vec!["k".to_string()]); // its build is now orphaned
    }

    #[test]
    fn gc_due_throttles() {
        let mut r = Registry::default();
        r.last_gc = 1000;
        assert!(!r.gc_due(1000 + GC_INTERVAL_SECS - 1));
        assert!(r.gc_due(1000 + GC_INTERVAL_SECS));
    }
}
