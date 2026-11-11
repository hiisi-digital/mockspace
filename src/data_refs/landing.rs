//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `Mock-Ref` lines of a landing: writing them for this clone, and checking a commit's.

/// The `Mock-Ref` trailer of each declared ref (or of `wanted` alone), naming the commit its
/// instance here is at. A landing made in this clone was gated against exactly those files, so
/// these are the lines its commit carries. Refused over an instance with changes the ref does not
/// hold, since the gate then read files no commit names. A ref not born yet has no commit to name
/// and no line: its instance holds no file a gate read, and `paired` takes a landing without the
/// line for a ref that had no commit when it landed.
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
        let Some(commit) = base.commit() else { continue };
        let pairing = crate::pairing_ref::pairing_of(&store, Some(commit.as_str())).map_err(|e| format!("{ns}: {e}"))?;
        out.push(crate::pairing::Trailer::new(ns, pairing).map_err(|e| format!("{ns}: {e}"))?.render());
    }
    Ok(out)
}

/// Checks that the commit `rev` carries a `Mock-Ref` trailer for every declared ref and that each
/// names a commit the ref holds, by hash or, where the hash is gone, by time and digest. A ref
/// that had no commit when `rev` was committed (the remote has none, or every commit of it was
/// authored after) needs no line, and is said to be skipped.
pub(super) fn paired(cfg: &crate::config::Config, rev: &str) -> Result<Vec<String>, Vec<String>> {
    use crate::pairing::Resolution;
    let message = std::process::Command::new("git")
        .arg("-C")
        .arg(&cfg.repo_root)
        .args(["log", "-1", "--format=%ct%n%B", "--end-of-options", rev, "--"])
        .output()
        .map_err(|e| vec![format!("running git log: {e}")])?;
    if !message.status.success() {
        return Err(vec![format!("`{rev}` is not a commit here: {}", String::from_utf8_lossy(&message.stderr).trim())]);
    }
    let text = String::from_utf8_lossy(&message.stdout);
    let (time, body) = text.split_once('\n').unwrap_or((&text, ""));
    let landed: i64 = time.trim().parse().map_err(|_| vec![format!("`{rev}` has no commit time git could give")])?;
    let trailers = crate::pairing::trailers(body).map_err(|e| vec![e])?;
    let (mut ok, mut why) = (Vec::new(), Vec::new());
    for d in &cfg.data_refs {
        let ns = d.namespace();
        let Some(trailer) = trailers.iter().find(|t| t.name() == ns) else {
            match unborn_when(d, cfg, landed) {
                Ok(true) => ok.push(format!("{ns}: not born when `{rev}` landed, so it carries no line for it")),
                Ok(false) => why.push(format!("{ns}: `{rev}` carries no `Mock-Ref` line for it")),
                Err(e) => why.push(format!("{ns}: `{rev}` carries no `Mock-Ref` line for it, and the ref cannot be read: {e}")),
            }
            continue;
        };
        let found = d
            .open(&cfg.repo_root)
            .and_then(|store| {
                // the remote's tip as it is now, history rewritten or not, read without moving the
                // instance's ref
                match store.fetch_for_reading().map_err(|e| e.to_string())? {
                    Some(tip) => crate::pairing_ref::resolve_at(&store, &tip, trailer.pairing(), crate::pairing::DEFAULT_WINDOW_SECS),
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
                why.push(format!("{ns}: the commit `{}` does not hold the files the trailer records", trailer.pairing().hash()));
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

/// Whether the ref of `d` had no commit at the time `landed`, in seconds since the epoch: the
/// remote has none, or the first commit of its history was authored later. A history the remote
/// rewrote keeps the author times, and one cut short by a shallow fetch cannot say, and is taken
/// as born.
fn unborn_when(d: &super::DataRefDecl, cfg: &crate::config::Config, landed: i64) -> Result<bool, String> {
    let store = d.open(&cfg.repo_root)?;
    let Some(tip) = store.fetch_for_reading().map_err(|e| e.to_string())? else { return Ok(true) };
    if store.is_shallow().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let history = store.history_at(&tip).map_err(|e| e.to_string())?;
    Ok(history.iter().map(|c| c.author_time).min().is_some_and(|first| first > landed))
}
