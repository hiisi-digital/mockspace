//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `Mock-Ref` lines of a landing: writing them for this clone, and checking a commit's.

use std::collections::BTreeSet;

use crate::pairing::{Against, Trailer};

/// The `Mock-Ref` trailer of each declared ref (or of `wanted` alone), naming the commit its
/// instance here is at. A landing made in this clone was gated against exactly those files, so
/// these are the lines its commit carries. Refused over an instance with changes the ref does not
/// hold, since the gate then read files no commit names. A ref not born yet has no commit to name,
/// and its line says so: the instance held no file a gate read.
pub fn trailer_lines(cfg: &crate::config::Config, wanted: Option<&str>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for d in cfg.data_refs.iter().filter(|d| wanted.is_none_or(|w| d.namespace() == w)) {
        let ns = d.namespace();
        let store = d.open(&cfg.repo_root).map_err(|e| format!("{ns}: {e}"))?;
        let inst = d.instance(&store, &cfg.mock_dir).map_err(|e| format!("{ns}: {e}"))?;
        let Some(base) = inst.base().map_err(|e| format!("{ns}: {e}"))? else {
            return Err(format!("{ns}: has no instance, so `instantiate` it"));
        };
        if inst.is_dirty().map_err(|e| format!("{ns}: {e}"))? {
            return Err(format!("{ns}: has changes the ref does not, so `commit` them before the landing is paired"));
        }
        let against = match base.commit() {
            Some(commit) => {
                Against::Commit(crate::pairing_ref::pairing_of(&store, Some(commit.as_str())).map_err(|e| format!("{ns}: {e}"))?)
            },
            None => Against::Unborn,
        };
        out.push(Trailer::new(ns, against).map_err(|e| format!("{ns}: {e}"))?.render());
    }
    Ok(out)
}

/// Checks that the commit `rev` carries a `Mock-Ref` trailer for every ref declared both here and
/// in the `mockspace.toml` of `rev` itself, and that each names a commit the ref holds, by hash
/// or, where the hash is gone, by time and digest. A line that says the ref was not born yet is
/// taken as the gate wrote it. No clock decides anything: a landing made before a ref was declared
/// is told by its own tree, which does not declare it.
pub(super) fn paired(cfg: &crate::config::Config, rev: &str) -> Result<Vec<String>, Vec<String>> {
    use crate::pairing::Resolution;
    let message = std::process::Command::new("git")
        .arg("-C")
        .arg(&cfg.repo_root)
        .args(["log", "-1", "--format=%B", "--end-of-options", rev, "--"])
        .output()
        .map_err(|e| vec![format!("running git log: {e}")])?;
    if !message.status.success() {
        return Err(vec![format!("`{rev}` is not a commit here: {}", String::from_utf8_lossy(&message.stderr).trim())]);
    }
    let body = String::from_utf8_lossy(&message.stdout);
    let trailers = crate::pairing::trailers(&body).map_err(|e| vec![e])?;
    let declared = declared_at(cfg, rev);
    let (mut ok, mut why) = (Vec::new(), Vec::new());
    for d in &cfg.data_refs {
        let ns = d.namespace();
        let Some(trailer) = trailers.iter().find(|t| t.name() == ns) else {
            if declared.as_ref().is_some_and(|names| !names.contains(ns)) {
                ok.push(format!("{ns}: not declared at `{rev}`, so it carries no line for it"));
            } else {
                why.push(format!("{ns}: `{rev}` carries no `Mock-Ref` line for it"));
            }
            continue;
        };
        let pairing = match trailer.against() {
            Against::Unborn => {
                ok.push(format!("{ns}: not born when `{rev}` was gated, as its line says"));
                continue;
            },
            Against::Commit(pairing) => pairing,
        };
        let found = d.open(&cfg.repo_root).and_then(|store| {
            // the remote's tip as it is now, history rewritten or not, read without moving the
            // instance's ref
            match store.fetch_for_reading().map_err(|e| e.to_string())? {
                Some(tip) => crate::pairing_ref::resolve_at(&store, &tip, pairing, crate::pairing::DEFAULT_WINDOW_SECS),
                None => Ok(Resolution::Missing),
            }
        });
        match found {
            Ok(Resolution::ByHash(c)) => ok.push(format!("{ns}: paired with {c}")),
            Ok(Resolution::ByTimeAndDigest(c)) => ok.push(format!("{ns}: paired with {c}, found by time and digest")),
            Ok(Resolution::Ambiguous(hits)) => {
                why.push(format!("{ns}: the recorded hash is gone and {} commits hold the same files", hits.len()));
            },
            Ok(Resolution::Mismatch { .. }) => {
                why.push(format!("{ns}: the commit `{}` does not hold the files the trailer records", pairing.hash()));
            },
            Ok(Resolution::Missing) => why.push(format!("{ns}: no commit of the ref holds the files the trailer records")),
            Ok(Resolution::Truncated) => {
                why.push(format!("{ns}: not found in the history fetched, which is cut short; fetch more of the ref"));
            },
            Err(e) => why.push(format!("{ns}: {e}")),
        }
    }
    if why.is_empty() { Ok(ok) } else { Err(why) }
}

/// The namespaces the `[[data_ref]]` tables of `rev`'s own `mockspace.toml` declare. `None`
/// where `rev` has no such file or it cannot be read, and every ref declared here is then held to
/// its line: only a config that says which refs it declares lets a landing go without one.
fn declared_at(cfg: &crate::config::Config, rev: &str) -> Option<BTreeSet<String>> {
    let file = cfg.mock_dir.join("mockspace.toml");
    let rel = file.strip_prefix(&cfg.repo_root).ok()?;
    let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    let shown = std::process::Command::new("git")
        .arg("-C")
        .arg(&cfg.repo_root)
        .args(["show", "--end-of-options", &format!("{rev}:{rel}")])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let doc = String::from_utf8(shown.stdout).ok()?.parse::<toml_edit::DocumentMut>().ok()?;
    let Some(tables) = doc.get("data_ref") else { return Some(BTreeSet::new()) };
    let tables = tables.as_array_of_tables()?;
    tables.iter().map(|t| t.get("namespace").and_then(|v| v.as_str()).map(str::to_owned)).collect()
}
