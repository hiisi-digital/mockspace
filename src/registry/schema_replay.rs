//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the schema check reads, as one key, so a run over the same inputs replays its verdict.
//!
//! The check is one `taplo check` process at the repository root. The generated config at the root
//! carries one rule and no top-level `include`, so taplo examines **every `.toml` under the root**,
//! `target/` and `.git/` and symlinked directories included, and the rule binds the generated schema
//! to the registry files. The key holds the taplo that runs (its `--version` output on both streams),
//! that config, the generated schemas and every one of those files.
//!
//! **Where taplo can read something the key does not hold, there is no key and the check runs.** That
//! is a config that is not exactly the generated one, a second config (`taplo.toml`), `TAPLO_CONFIG`
//! or `RUST_LOG` in the environment (the latter changes the log line the empty-run check reads), and
//! any keyed file that might bind a schema of its own: one with a `#:` comment in it, one that does not
//! parse, or one with a root key that decodes to `$schema`. A `file://` reference inside the generated
//! schema is not followed; the schemas are rewritten before every check. Only a
//! pass is remembered ([`PASSED`]); a failure, a taplo that is absent and a run that examined no
//! files are all judged again, so none of them can be replayed as green.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_lint_rules::verdict::VerdictCache;

/// What is saved under a key whose check passed.
pub(super) const PASSED: &str = "taplo-check-passed-v1";

/// The line the generated config binds its schema with.
const SCHEMA_LINE: &str = "schema.path = \"target/mockspace/registry-schemas/registry.schema.json\"";

/// The key for one run, or `None` when taplo could read what the key does not hold or an input
/// cannot be read, which is a run with no replay. `taplo_version` is what the validator printed for
/// `--version`; `env_changes_taplo` says `TAPLO_CONFIG` or `RUST_LOG` is set.
pub(super) fn input_key(
    cache: &dyn VerdictCache,
    repo_root: &Path,
    taplo_version: &[u8],
    env_changes_taplo: bool,
) -> Option<String> {
    if env_changes_taplo || repo_root.join("taplo.toml").exists() {
        return None;
    }
    let config = fs::read_to_string(repo_root.join(".taplo.toml")).ok()?;
    if !is_the_generated_config(&config) {
        return None;
    }
    let mut named: Vec<(String, Vec<u8>)> = Vec::new();
    let mut ancestors: Vec<PathBuf> = Vec::new();
    collect(repo_root, repo_root, &mut ancestors, &mut named)?;
    if named.iter().any(|(_, bytes)| may_bind_a_schema(bytes)) {
        return None;
    }
    let schemas = crate::build_dir::target_dir(repo_root).join("mockspace").join("registry-schemas");
    for entry in fs::read_dir(&schemas).ok()? {
        let entry = entry.ok()?;
        let name = format!("schemas/{}", entry.file_name().to_string_lossy());
        named.push((name, fs::read(entry.path()).ok()?));
    }
    named.sort();
    let mut parts: Vec<&[u8]> = vec![b"taplo-check-v2", taplo_version];
    for (name, bytes) in &named {
        parts.push(name.as_bytes());
        parts.push(bytes);
    }
    Some(cache.digest(&parts))
}

/// Whether `config` is what the generator writes: comments, then one rule with an `include` and the
/// generated schema, and nothing else. A marker alone would let a hand edit keep it.
fn is_the_generated_config(config: &str) -> bool {
    let lines: Vec<&str> = config.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    matches!(lines.as_slice(), ["[[rule]]", include, schema] if include.starts_with("include = [") && *schema == SCHEMA_LINE)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether a keyed file might bind a schema of its own. Refused broadly rather than by taplo's
/// grammar: any `#:` comment (a directive, however it is spaced) and any file that does not parse
/// or carries a root key that decodes to `$schema` (however it is escaped).
fn may_bind_a_schema(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else { return true };
    if text.contains("#:") {
        return true;
    }
    text.parse::<toml_edit::DocumentMut>().map_or(true, |doc| doc.contains_key("$schema"))
}

/// Every `.toml` under `dir` with its path from `root`, following symlinks as taplo's walk does and
/// keying every path it is reached by: a directory seen again under another name is entered again,
/// since taplo checks the files under both. Only a directory that is one of its own ancestors is
/// not entered, which ends a loop.
fn collect(root: &Path, dir: &Path, ancestors: &mut Vec<PathBuf>, out: &mut Vec<(String, Vec<u8>)>) -> Option<()> {
    let here = fs::canonicalize(dir).ok()?;
    if ancestors.contains(&here) {
        return Some(());
    }
    ancestors.push(here);
    for entry in fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let kind = fs::metadata(&path).ok()?;
        if kind.is_dir() {
            collect(root, &path, ancestors, out)?;
        } else if path.extension().is_some_and(|e| e == "toml") {
            let rel = path.strip_prefix(root).ok()?.to_string_lossy().into_owned();
            out.push((rel, fs::read(&path).ok()?));
        }
    }
    ancestors.pop();
    Some(())
}

#[cfg(test)]
#[path = "schema_replay_tests.rs"]
mod tests;
