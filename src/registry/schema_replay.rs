//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the schema check reads, as one key, so a run over the same inputs replays its verdict.
//!
//! The check is one `taplo check` process at the repository root. When the config at the root is the
//! one mockspace generated, what it can depend on is the taplo that runs, that config, the generated
//! schemas it validates against and the TOML files it examines. The key holds those four: **every
//! `.toml` file under the root except `.git` and the root `target/`**, a superset of what any
//! `include` selects, so an unrelated manifest moving costs a replay and never a stale pass.
//!
//! **Where taplo can read something the key does not hold, there is no key and the check runs.** That
//! is a config the project wrote itself (it can name any schema path), a second config
//! (`taplo.toml`), `TAPLO_CONFIG` in the environment, and a `#:schema` directive in a keyed file.
//! Only a pass is remembered ([`PASSED`]); a failure, a taplo that is absent and a run that examined
//! no files are all judged again, so none of them can be replayed as green.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_lint_rules::verdict::VerdictCache;

use crate::render_design::GENERATED_MARKER;

/// What is saved under a key whose check passed.
pub(super) const PASSED: &str = "taplo-check-passed-v1";

/// The key for one run, or `None` when taplo could read what the key does not hold or an input
/// cannot be read, which is a run with no replay. `taplo_version` is what the validator printed
/// for `--version`; `config_from_env` says `TAPLO_CONFIG` is set.
pub(super) fn input_key(
    cache: &dyn VerdictCache,
    repo_root: &Path,
    taplo_version: &[u8],
    config_from_env: bool,
) -> Option<String> {
    if config_from_env || repo_root.join("taplo.toml").exists() {
        return None;
    }
    let config = fs::read_to_string(repo_root.join(".taplo.toml")).ok()?;
    if !config.contains(GENERATED_MARKER) {
        return None;
    }
    let mut named: Vec<(String, Vec<u8>)> = Vec::new();
    collect(repo_root, repo_root, &mut named)?;
    let schemas = crate::build_dir::target_dir(repo_root).join("mockspace").join("registry-schemas");
    for entry in fs::read_dir(&schemas).ok()? {
        let entry = entry.ok()?;
        let name = format!("schemas/{}", entry.file_name().to_string_lossy());
        named.push((name, fs::read(entry.path()).ok()?));
    }
    if named.iter().any(|(_, bytes)| contains(bytes, b"#:schema")) {
        return None;
    }
    named.sort();
    let mut parts: Vec<&[u8]> = vec![b"taplo-check-v1", taplo_version];
    for (name, bytes) in &named {
        parts.push(name.as_bytes());
        parts.push(bytes);
    }
    Some(cache.digest(&parts))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Every `.toml` under `dir` with its path from `root`, leaving `.git` and the root `target/`.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Option<()> {
    for entry in fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path: PathBuf = entry.path();
        let kind = entry.file_type().ok()?;
        if kind.is_dir() {
            let name = entry.file_name();
            if name == ".git" || (dir == root && name == "target") {
                continue;
            }
            collect(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "toml") {
            let rel = path.strip_prefix(root).ok()?.to_string_lossy().into_owned();
            out.push((rel, fs::read(&path).ok()?));
        }
    }
    Some(())
}

#[cfg(test)]
#[path = "schema_replay_tests.rs"]
mod tests;
