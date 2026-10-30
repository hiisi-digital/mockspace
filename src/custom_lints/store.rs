//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The shared store the generated lint crates live in.
//!
//! One directory per key under a base outside every worktree. The key hashes
//! what makes two builds differ (the repo's identity, the mock directory below
//! the repo root, the engine's lint-rules dependency and the resolved pack
//! revisions) and nothing about where a worktree sits, so the worktrees of one
//! repo at the same revisions are one cargo package and share its dependency
//! builds.
//!
//! A key directory is one crate, written by whichever worktree runs, so every
//! run takes the directory's lock for the write, the build and the load. The
//! worktree that wrote it last is recorded as its owner, and `mock test` uses
//! that to test only a crate that is still its own.
//!
//! A new key appears whenever a pack tip or the lint-rules revision moves, and
//! the old one is never used again, so [`reap`] removes the key directories
//! nobody can come back to.

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::config::Config;

const LOCK: &str = ".lock";
const OWNER: &str = ".owner";
const USED: &str = ".used";
const POINTER: &str = "shared-dir";

/// How long a key directory may go unused before [`reap`] removes it.
///
/// A key moves on every pack push and every lint-rules revision, and each
/// holds about 85 MB of build that nothing refers to once the key has moved, so
/// the number has to be short enough for those to clear within a sprint. It
/// also has to outlast a branch left alone for a while, whose first run after
/// the gap would otherwise pay a cold release build of the whole dependency
/// graph. Fourteen days sits between the two: a branch untouched for two weeks
/// rebuilds once, and a key abandoned by a moved pin is gone in two weeks. A
/// removal is only ever a rebuild, never a wrong answer, so the figure is a
/// disk-for-time trade and not a correctness bound.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// Where the shared generated lint crates live, one directory per key.
///
/// Under the machine's shared cargo target directory when one is set, which is
/// where the rest of the build already is, and otherwise in the user's cache.
/// A relative target directory is read against the repo root, so the answer
/// does not move with the directory the engine happens to run in.
pub(crate) fn shared_base(repo_root: &Path) -> Result<PathBuf, String> {
    base_from(
        crate::build_dir::inherited_target_dir(),
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
        repo_root,
    )
}

/// [`shared_base`] with the environment passed in, so it is testable without a
/// test changing the process environment under every other test.
///
/// There is no fallback to the temp directory. A directory every user can write
/// to is a place another user can plant a `Cargo.toml` that this engine then
/// builds and loads, so a machine with no home gets an error naming what to set.
fn base_from(
    target: Option<OsString>,
    xdg_cache: Option<OsString>,
    home: Option<OsString>,
    repo_root: &Path,
) -> Result<PathBuf, String> {
    if let Some(target) = target.filter(|t| !t.is_empty()) {
        let target = PathBuf::from(target);
        let target = if target.is_absolute() { target } else { repo_root.join(target) };
        return Ok(target.join("mockspace-lints"));
    }
    // The XDG specification says a relative value is invalid and to be ignored.
    let cache = xdg_cache
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
                .filter(|p| p.is_absolute())
        });
    match cache {
        Some(cache) => Ok(cache.join("mockspace").join("lints")),
        None => {
            Err(
                "nowhere to put the generated lint crate: none of CARGO_TARGET_DIR, an absolute \
             XDG_CACHE_HOME or an absolute HOME is set. The crate is built and loaded, so it \
             cannot live in a directory every user can write to; set one of them."
                    .to_string(),
            )
        },
    }
}

/// Where the generated lint crate's manifest goes: `<base>/<key>/Cargo.toml`,
/// outside every worktree.
///
/// `packs` are the specs cargo will be handed, a branch already replaced by the
/// rev of its tip (see [`resolve_packs`]). A changed rev is a new key and so a
/// separate build; the same revs from any worktree are the same cargo package,
/// which is the sharing. Pure apart from reading the repo's git config.
///
/// `scope` is the one tool a build is cut to (`None` for the whole content),
/// and it is part of the key: a build of one tool is not the build of every
/// lint and tool, and the two must not overwrite each other's generated crate.
pub(crate) fn lints_manifest_path(
    base: &Path,
    cfg: &Config,
    lint_rules_dep: &str,
    packs: &[(String, String)],
    scope: Option<&str>,
) -> PathBuf {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir);
    let key = lint_crate_key(
        &repo_identity(&cfg.repo_root),
        mock_rel,
        lint_rules_dep,
        packs,
        scope,
    );
    base.join(format!("{key:016x}")).join("Cargo.toml")
}

/// Each pack's spec with a branch replaced by the rev of its tip.
///
/// Called once per run: the note it prints when a tip could not be read is
/// printed once, and the key and the written manifest cannot disagree about a
/// tip that moved between two reads.
pub(crate) fn resolve_packs(
    packs: &[(String, String)],
    pins: &Path,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Vec<(String, String)> {
    packs
        .iter()
        .map(|(name, spec)| {
            let said = format!("lint pack `{name}`");
            (
                name.clone(),
                crate::pack_pin::spec_for_cargo(&said, spec, pins, resolve),
            )
        })
        .collect()
}

/// FNV-1a rather than std's hasher, whose output may change between releases:
/// the key names a directory that outlives the binary that made it.
fn lint_crate_key(
    identity: &str,
    mock_rel: &Path,
    lint_rules_dep: &str,
    packs: &[(String, String)],
    scope: Option<&str>,
) -> u64 {
    let mut text = format!("{identity}\n{}\n{lint_rules_dep}\n", mock_rel.display());
    for (name, spec) in packs {
        text.push_str(&format!("{name}={spec}\n"));
    }
    // Appended only when set, so the key of every whole build is the key it
    // always had and no existing build is orphaned.
    if let Some(tool) = scope {
        text.push_str(&format!("scope={tool}\n"));
    }
    crate::pack_pin::fnv1a(text.as_bytes())
}

/// What names the repo across worktrees and clones: its `origin` url with the
/// scheme, user and `.git` taken off, else the git directory every worktree of
/// the clone shares, else the path.
fn repo_identity(repo_root: &Path) -> String {
    if let Some(url) = git_line(repo_root, &["config", "--get", "remote.origin.url"]) {
        return format!("remote:{}", normalise_remote(&url));
    }
    if let Some(dir) = git_line(repo_root, &[
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]) {
        return format!("git-dir:{dir}");
    }
    format!("path:{}", repo_root.display())
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .env("LC_ALL", "C")
        .current_dir(dir)
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// `https://h/o/r.git`, `ssh://git@h/o/r` and `git@h:o/r.git` are one remote.
fn normalise_remote(url: &str) -> String {
    let mut s = url.trim().to_lowercase();
    let mut had_scheme = false;
    if let Some(i) = s.find("://") {
        s = s[i + 3 ..].to_string();
        had_scheme = true;
    }
    if let Some(at) = s.find('@')
        && s.find('/').is_none_or(|slash| at < slash)
    {
        s = s[at + 1 ..].to_string();
    }
    if !had_scheme
        && let Some(colon) = s.find(':')
        && !s[.. colon].contains('/')
    {
        s.replace_range(colon ..= colon, "/");
    }
    s.trim_end_matches('/').trim_end_matches(".git").to_string()
}

/// A pack's `path` made absolute against `from`, where it was written to be
/// read from, and folded to its shortest form. Every other spec is returned as
/// it came.
pub(crate) fn absolutize_path_pack(spec: &str, from: &Path) -> String {
    let Ok(mut doc) = format!("p = {spec}").parse::<toml_edit::DocumentMut>() else {
        return spec.to_string();
    };
    let Some(table) = doc.get_mut("p").and_then(|i| i.as_inline_table_mut()) else {
        return spec.to_string();
    };
    let Some(path) = table
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return spec.to_string();
    };
    if Path::new(&path).is_absolute() {
        return spec.to_string();
    }
    let abs = crate::build_dir::lexically_normal(&from.join(path))
        .to_string_lossy()
        .replace('\\', "/");
    table.insert("path", toml_edit::Value::from(abs));
    table.fmt();
    table.to_string().trim().to_string()
}

/// What goes wrong when the lock cannot be had, said once for both places it
/// can fail.
fn lock_hint(gen_dir: &Path) -> String {
    format!(
        "the shared lint crate directory {} needs a writable filesystem that supports file \
         locks; point XDG_CACHE_HOME or CARGO_TARGET_DIR at one that does",
        gen_dir.display()
    )
}

/// Create `gen_dir` and hold its lock for this run. Released when the file is
/// dropped.
///
/// The lock is only worth having if the file locked is the file [`reap`] would
/// delete, so once held it is checked still to be the one at the path. A reaper
/// that removed the directory between our open and our lock leaves us holding
/// an unlinked file, and a second process locking the recreated one would then
/// run beside us; here that loops and starts again.
pub(crate) fn lock_dir(gen_dir: &Path) -> Result<File, String> {
    let path = gen_dir.join(LOCK);
    loop {
        std::fs::create_dir_all(gen_dir).map_err(|e| {
            format!(
                "could not create {}: {e}; {}",
                gen_dir.display(),
                lock_hint(gen_dir)
            )
        })?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| {
                format!(
                    "could not open {}: {e}; {}",
                    path.display(),
                    lock_hint(gen_dir)
                )
            })?;
        #[cfg(test)]
        if knobs::skips_the_lock(gen_dir) {
            return Ok(file);
        }
        file.lock().map_err(|e| {
            format!(
                "could not lock {}: {e}; {}",
                path.display(),
                lock_hint(gen_dir)
            )
        })?;
        if is_the_file_at(&file, &path) {
            return Ok(file);
        }
    }
}

#[cfg(unix)]
fn is_the_file_at(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(held), Ok(there)) => held.dev() == there.dev() && held.ino() == there.ino(),
        _ => false,
    }
}

// Without inodes there is nothing to compare, and the reaper's removal of a
// directory that is locked is refused by the platform on these anyway.
#[cfg(not(unix))]
fn is_the_file_at(_file: &File, path: &Path) -> bool {
    path.is_file()
}

/// Record which worktree last wrote the shared crate, and where, so `mock test`
/// in a worktree can tell the crate is its own.
///
/// A build cut to one tool (`scope`) records its owner, which the reaper reads, and never the pointer: `mock
/// test` takes the lints it runs from the pointer, and a crate of one tool has none, so a pointer moved to it
/// is a green test run over no lints.
pub(crate) fn record_owner(
    gen_dir: &Path,
    mock_dir: &Path,
    scope: Option<&str>,
) -> Result<(), String> {
    super::write_if_changed(&gen_dir.join(OWNER), &mock_dir.display().to_string())?;
    if scope.is_some() {
        return Ok(());
    }
    let local = crate::build_dir::ensure_under_target(mock_dir, &[crate::build_dir::LINT_CRATE]);
    super::write_if_changed(&local.join(POINTER), &gen_dir.display().to_string())
}

/// The tool names more than one source registers, as the last whole build saw them, one per line.
const DUPLICATES: &str = "duplicate-tools";

/// Record, beside the pointer, which tool names the last whole build found registered more than once. A
/// run of one tool does not build the packs, so it cannot ask them; it reads this instead.
pub(crate) fn record_duplicates(mock_dir: &Path, names: &[String]) -> Result<(), String> {
    let local = crate::build_dir::ensure_under_target(mock_dir, &[crate::build_dir::LINT_CRATE]);
    super::write_if_changed(&local.join(DUPLICATES), &names.join("\n"))
}

/// What the last whole build recorded: `None` when none has (or the file is gone), `Some(empty)` when it
/// found no duplicate.
pub(crate) fn duplicates_of(mock_dir: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(crate::build_dir::lint_crate_dir(mock_dir).join(DUPLICATES)).ok()?;
    Some(text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// What a run of one tool does, given the record of the last whole build.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScopedPlan {
    /// The tool's name is registered twice: refused, as every whole run refuses it.
    Refuse(Vec<String>),
    /// Nothing recorded yet: one whole build, which writes the record.
    Whole,
    /// Recorded and clean for this tool: build that tool alone.
    OneTool,
}

pub(crate) fn plan_scoped(recorded: Option<Vec<String>>, tool: &str) -> ScopedPlan {
    match recorded {
        None => ScopedPlan::Whole,
        Some(d) if d.iter().any(|n| n == tool) => ScopedPlan::Refuse(d),
        Some(_) => ScopedPlan::OneTool,
    }
}

/// Stamp the directory as used now. Written on every run, unlike the owner,
/// because its modification time is the whole of what it records.
pub(crate) fn touch_used(gen_dir: &Path) -> Result<(), String> {
    let path = gen_dir.join(USED);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    std::fs::write(&path, now.to_string())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Where the generated crate stands for one worktree.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Generated {
    /// This worktree never wrote one, or what it wrote has been removed.
    Absent,
    /// The crate is at this directory and this worktree wrote it last.
    Yours(PathBuf),
    /// The crate is at this directory and another worktree has rewritten it
    /// since, to its own lint sources. Testing it from here would test theirs.
    Taken {
        dir:   PathBuf,
        owner: String,
    },
}

/// Which of the three the crate this worktree last wrote is in now.
pub(crate) fn generated_dir_of(mock_dir: &Path) -> Generated {
    let pointer = crate::build_dir::lint_crate_dir(mock_dir).join(POINTER);
    let Ok(dir) = std::fs::read_to_string(pointer) else {
        return Generated::Absent;
    };
    let dir = PathBuf::from(dir.trim());
    let Ok(owner) = std::fs::read_to_string(dir.join(OWNER)) else {
        return Generated::Absent;
    };
    if !dir.join("Cargo.toml").is_file() {
        return Generated::Absent;
    }
    let owner = owner.trim().to_string();
    if owner == mock_dir.display().to_string() {
        Generated::Yours(dir)
    } else {
        Generated::Taken {
            dir,
            owner,
        }
    }
}

/// A key directory's name: sixteen lowercase hex digits, as the key is written.
/// Anything else under the base (the pack pins, a stray file) is not ours to
/// remove.
fn is_key_name(name: &str) -> bool {
    name.len() == 16
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a' ..= b'f').contains(&b))
}

/// Whether a key directory has been abandoned: the worktree that wrote it last
/// no longer exists, or nothing has used it for longer than `stale_after`.
///
/// A directory with no recorded owner is judged on age alone, which is what
/// keeps one a peer has created and not yet written from being taken.
fn abandoned(dir: &Path, now: SystemTime, stale_after: Duration) -> bool {
    if let Ok(owner) = std::fs::read_to_string(dir.join(OWNER))
        && !Path::new(owner.trim()).exists()
    {
        return true;
    }
    let last_used = std::fs::metadata(dir.join(USED))
        .or_else(|_| std::fs::metadata(dir))
        .and_then(|m| m.modified());
    match last_used {
        Ok(at) => now.duration_since(at).is_ok_and(|age| age > stale_after),
        // Not knowing is not a reason to delete.
        Err(_) => false,
    }
}

/// Remove the abandoned key directories under `base`, except `keep`, and return
/// the ones removed.
///
/// A key in use is never removed: each candidate's lock is taken without
/// waiting, and one that is held is skipped. The abandonment is judged only once
/// the lock is held, so a run that started between the listing and the lock
/// has already stamped the directory by then. Removal is best effort; a
/// directory that will not go is tried again on the next run.
pub(crate) fn reap(
    base: &Path,
    keep: &Path,
    now: SystemTime,
    stale_after: Duration,
) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir == keep || !is_key_name(&entry.file_name().to_string_lossy()) || !dir.is_dir() {
            continue;
        }
        // No lock file means no run has ever locked it, so there is no holder
        // to wait for; the age test then has to stand on the directory alone.
        let held = match File::open(dir.join(LOCK)) {
            Ok(file) => {
                match file.try_lock() {
                    Ok(()) => Some(file),
                    // Held by a run, or not lockable at all: leave it.
                    Err(_) => continue,
                }
            },
            Err(_) => None,
        };
        if abandoned(&dir, now, stale_after) && std::fs::remove_dir_all(&dir).is_ok() {
            removed.push(dir);
        }
        drop(held);
    }
    removed
}

/// Hooks the end-to-end tests use to make a race happen on purpose instead of
/// hoping for one, keyed by directory so a test that sets one cannot disturb
/// another running beside it. Absent from a build that is not a test.
#[cfg(test)]
pub(crate) mod knobs {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    struct Knob {
        under:     PathBuf,
        skip_lock: bool,
        pause:     Duration,
    }

    static KNOBS: Mutex<Vec<Knob>> = Mutex::new(Vec::new());

    /// For every crate directory under `under`: optionally take no lock, and
    /// sleep `pause` between writing the crate and building it.
    pub(crate) fn set(under: &Path, skip_lock: bool, pause: Duration) {
        KNOBS.lock().unwrap().push(Knob {
            under: under.to_path_buf(),
            skip_lock,
            pause,
        });
    }

    pub(crate) fn skips_the_lock(dir: &Path) -> bool {
        KNOBS
            .lock()
            .unwrap()
            .iter()
            .any(|k| dir.starts_with(&k.under) && k.skip_lock)
    }

    pub(crate) fn pause_between_write_and_build(dir: &Path) {
        let pause = KNOBS
            .lock()
            .unwrap()
            .iter()
            .filter(|k| dir.starts_with(&k.under))
            .map(|k| k.pause)
            .max();
        if let Some(pause) = pause {
            std::thread::sleep(pause);
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "store_e2e_tests.rs"]
mod e2e;
