//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `[lint-crates]` pack pinned by branch, built from the branch's tip.
//!
//! Cargo resolves `branch = "dev"` once, into the generated crate's lockfile,
//! and never again unless somebody runs `cargo update` against a manifest
//! nobody thinks of as theirs. So a repository that pinned a pack by branch
//! gated on whatever commit was the tip the first time it built, for as long
//! as that lockfile lived, while a fresh clone of the same repository resolved
//! today's tip and gated on something else. Nothing reported either.
//!
//! The spec handed to cargo is rewritten here instead: the branch is resolved
//! with `git ls-remote` and written as `rev`, so the manifest names the commit
//! and cargo re-resolves whenever that commit moves. A resolution is kept
//! beside the generated crate for [`TIP_TTL`], so a run does not pay a network
//! round trip per pack every time it gates a commit.
//!
//! Where the remote cannot be reached, the last resolution is used at any age
//! and the run says how old it is. Where there has never been one, the spec
//! goes to cargo unchanged, which is what happened before this existed, and
//! the run says that too.
//!
//! This runs inside the commit gate, so asking the remote has a deadline,
//! [`ASK_DEADLINE`], and never prompts: a network dropping packets or an ssh
//! key wanting its passphrase is a remote that did not answer, and the
//! fallbacks above take it from there.
//!
//! renki resolves the launcher's own branch pins the same way, with the same
//! hour and the same cache file shape. The engine does not depend on renki,
//! so it carries its own.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, InlineTable, Value};

/// How long a resolved tip is taken as the branch's current one.
pub(crate) const TIP_TTL: Duration = Duration::from_secs(60 * 60);

/// How long the remote is given to answer before it counts as unreachable.
pub(crate) const ASK_DEADLINE: Duration = Duration::from_secs(5);

/// A branch and the repository it lives in, read out of one pack's spec.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BranchPin {
    pub(crate) git:    String,
    pub(crate) branch: String,
}

/// What a spec was pinned to, and how the run learned it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pinned {
    /// Not a branch pin at all: a `rev`, a `tag`, a version or a path.
    Untouched,
    /// Resolved from the remote just now, or from a resolution younger than
    /// [`TIP_TTL`].
    Current { rev: String },
    /// The remote did not answer, so an older resolution stands in.
    Stale { rev: String, age: Duration, why: String },
    /// The remote did not answer and nothing was ever resolved, so cargo gets
    /// the branch and the lockfile decides.
    Unresolved { why: String },
}

/// The branch pin a spec carries, if it carries one and nothing fixed beside it.
///
/// A spec naming `rev` or `tag` as well is left alone, since cargo refuses the
/// combination and saying so is cargo's job.
pub(crate) fn branch_pin(spec: &str) -> Option<BranchPin> {
    let table = inline_table(spec)?;
    if table.contains_key("rev") || table.contains_key("tag") {
        return None;
    }
    Some(BranchPin {
        git:    table.get("git")?.as_str()?.to_string(),
        branch: table.get("branch")?.as_str()?.to_string(),
    })
}

/// The spec with its branch replaced by `rev`, every other key kept.
pub(crate) fn with_rev(spec: &str, rev: &str) -> Option<String> {
    let mut table = inline_table(spec)?;
    table.remove("branch")?;
    table.insert("rev", Value::from(rev));
    table.fmt();
    Some(table.to_string().trim().to_string())
}

/// Resolve one pack's pin, reading and writing the cache under `cache`.
///
/// `resolve` asks the remote for a branch's tip; it is a parameter so the
/// cache and the fallbacks can be driven without a network.
pub(crate) fn pin(
    spec: &str,
    cache: &Path,
    now: SystemTime,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Pinned {
    let Some(p) = branch_pin(spec) else {
        return Pinned::Untouched;
    };
    let file = cache_file(cache, &p);
    let held = read_resolution(&file);
    // A stamp from the future, a clock set back or skewed, is not fresh: it
    // would otherwise read as age zero until the clock caught up with it.
    if let Some((at, rev)) = &held
        && *at <= unix(now)
        && age(*at, now) <= TIP_TTL
    {
        return Pinned::Current { rev: rev.clone() };
    }
    match resolve(&p.git, &p.branch) {
        Ok(rev) => {
            write_resolution(&file, now, &rev);
            Pinned::Current { rev }
        }
        Err(why) => match held {
            Some((at, rev)) => Pinned::Stale {
                rev,
                age: age(at, now),
                why,
            },
            None => Pinned::Unresolved { why },
        },
    }
}

/// The spec cargo is handed for one pack, saying on stderr where it had to
/// fall back.
pub(crate) fn spec_for_cargo(
    name: &str,
    spec: &str,
    cache: &Path,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> String {
    let pinned = pin(spec, cache, SystemTime::now(), resolve);
    if let Some(note) = fallback_note(name, &pinned) {
        eprintln!("{note}");
    }
    match pinned {
        Pinned::Untouched | Pinned::Unresolved { .. } => spec.to_string(),
        Pinned::Current { rev } | Pinned::Stale { rev, .. } => {
            with_rev(spec, &rev).unwrap_or_else(|| spec.to_string())
        }
    }
}

/// What the run says when it had to fall back, and nothing when it did not.
pub(crate) fn fallback_note(name: &str, pinned: &Pinned) -> Option<String> {
    match pinned {
        Pinned::Untouched | Pinned::Current { .. } => None,
        Pinned::Stale { rev, age, why } => Some(format!(
            "mock: lint pack `{name}`: the branch tip could not be read ({why}), so the \
             pack is built at {rev}, resolved {} minutes ago",
            age.as_secs() / 60
        )),
        Pinned::Unresolved { why } => Some(format!(
            "mock: lint pack `{name}`: the branch tip could not be read ({why}) and was \
             never resolved here, so the pack is built at whatever the lockfile holds"
        )),
    }
}

/// The tip of `branch` on `url`, by `git ls-remote`, within [`ASK_DEADLINE`].
///
/// The full ref, since a bare name also matches a tag of the same name and
/// which one answers first is the remote's listing order.
pub(crate) fn ls_remote_head(url: &str, branch: &str) -> Result<String, String> {
    let refspec = format!("refs/heads/{branch}");
    let mut git = Command::new("git");
    git.args(["ls-remote", url, &refspec])
        .env("GIT_TERMINAL_PROMPT", "0");
    // BatchMode stops ssh asking for a passphrase or a host key, and is only
    // set where the clone has not already said how ssh is to be run.
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        git.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let what = format!("git ls-remote {url} {refspec}");
    let out = run_within(git, ASK_DEADLINE, &what)?;
    if !out.status.success() {
        return Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    tip_from_listing(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| format!("{url} has no branch `{branch}`"))
}

/// The object name on the first line of an `ls-remote` listing, if it is one:
/// forty hex digits for a SHA-1 repository, sixty-four for a SHA-256 one.
pub(crate) fn tip_from_listing(listing: &str) -> Option<String> {
    let rev = listing.lines().next()?.split_whitespace().next()?;
    is_object_name(rev).then(|| rev.to_string())
}

fn is_object_name(rev: &str) -> bool {
    matches!(rev.len(), 40 | 64) && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Run `cmd` to completion, or kill it once `deadline` has passed.
///
/// Both pipes are drained on their own threads, so a child writing more than a
/// pipe holds cannot stall on a parent that is only waiting for it to exit.
pub(crate) fn run_within(mut cmd: Command, deadline: Duration, what: &str) -> Result<Output, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {what}: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{what} did not answer within {} seconds",
                    deadline.as_secs_f32()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("could not wait for {what}: {e}")),
        }
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn inline_table(spec: &str) -> Option<InlineTable> {
    let doc: DocumentMut = format!("p = {spec}").parse().ok()?;
    doc.get("p")?.as_inline_table().cloned()
}

pub(crate) fn cache_file(cache: &Path, p: &BranchPin) -> PathBuf {
    // Two fields hashed as one string with a separator neither can carry, so
    // `a`+`bc` and `ab`+`c` are two files. FNV-1a rather than std's hasher,
    // whose output is allowed to change between releases: a toolchain bump
    // would rename every file and lose the offline fallback with it.
    cache.join(format!("{:016x}", fnv1a(format!("{}\n{}", p.git, p.branch).as_bytes())))
}

pub(crate) const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

fn read_resolution(file: &Path) -> Option<(u64, String)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    let at = lines.next()?.trim().parse().ok()?;
    let rev = lines.next()?.trim();
    is_object_name(rev).then(|| (at, rev.to_string()))
}

/// Best effort: a resolution that cannot be kept costs the next run a round
/// trip and nothing else.
fn write_resolution(file: &Path, now: SystemTime, rev: &str) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("{}\n{rev}\n", unix(now)));
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn age(at: u64, now: SystemTime) -> Duration {
    Duration::from_secs(unix(now).saturating_sub(at))
}

#[cfg(test)]
#[path = "pack_pin_tests.rs"]
mod tests;
