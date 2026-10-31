//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock round-id` and `cargo mock rename-round`.
//!
//! The shape of an id and why is in [`lint_rules::round_id`]. A round's files are
//! named by hand, so the first half of the fix is a command that prints an id
//! no other branch is likely to pick: the minute plus a tag drawn from where the
//! round is opened, walked past anything the working tree, the round tags or the
//! trunk already holds. The second half is the repair for two rounds that still
//! share an id, which `merge-round-meta` (see `round_merge`) refuses to merge:
//! one side is renamed on its own branch, before the merge.

use std::collections::BTreeSet;

use lint_rules::round_id;

use super::*;

/// The minute, `YYYYMMDDHHMM`, from the system clock.
fn stamp_now() -> Option<String> {
    let out = Command::new("date").arg("+%Y%m%d%H%M").output().ok()?;
    let stamp = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (stamp.len() == 12 && stamp.bytes().all(|b| b.is_ascii_digit())).then_some(stamp)
}

fn git_line(cfg: &Config, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(&cfg.repo_root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Where a round is being opened from: what makes two sessions in the same
/// minute draw different tags. None of it is secret or stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    pub(crate) branch: String,
    pub(crate) host:   String,
    pub(crate) pid:    u32,
    pub(crate) nanos:  u128,
}

impl Origin {
    /// Read from the clone and the process.
    pub(crate) fn here(cfg: &Config) -> Origin {
        let branch = git_line(cfg, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
        let host = pick_host(std::env::var("HOSTNAME").ok(), || {
            Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        });
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Origin {
            branch,
            host,
            pid: std::process::id(),
            nanos,
        }
    }

    /// The text the tag is drawn from.
    pub(crate) fn seed(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.branch,
            self.host.trim(),
            self.pid,
            self.nanos
        )
    }
}

/// The environment's `HOSTNAME` when it has one, else what `hostname` prints.
fn pick_host(env: Option<String>, command: impl FnOnce() -> Option<String>) -> String {
    env.or_else(command).unwrap_or_default()
}

fn seed(cfg: &Config) -> String {
    Origin::here(cfg).seed()
}

/// Ids and round directory names present in `names`, both the whole name and
/// the id it starts with.
fn note_names<'a>(into: &mut BTreeSet<String>, names: impl Iterator<Item = &'a str>) {
    for name in names {
        if let Some(id) = round_id::base_id(name) {
            into.insert(id.to_string());
            into.insert(name.to_string());
        }
    }
}

/// Every round id this repository can see: the flat files and closed
/// directories of `design_rounds/`, the `round/<dir>/...` tags, and the same
/// directory on the remote trunks that are fetched. Offline, the last is empty,
/// and the tag walk plus the tag's own entropy is what is left.
pub(crate) fn taken_ids(cfg: &Config) -> BTreeSet<String> {
    let mut taken = BTreeSet::new();
    let dr = design_rounds_dir(cfg);
    if let Ok(entries) = fs::read_dir(&dr) {
        let names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        note_names(&mut taken, names.iter().map(String::as_str));
    }
    if let Some(tags) = git_line(cfg, &["tag", "-l", "round/*"]) {
        note_names(
            &mut taken,
            tags.lines().filter_map(|t| t.split('/').nth(1)),
        );
    }
    let rel = pathdiff(&cfg.repo_root, &dr);
    for trunk in ["origin/HEAD", "origin/dev", "origin/main"] {
        let spec = format!("{trunk}:{rel}");
        if let Some(listing) = git_line(cfg, &["ls-tree", "--name-only", &spec]) {
            note_names(&mut taken, listing.lines());
        }
    }
    taken
}

/// The trunk that already holds the closed round directory `old` exactly as it is
/// committed here, if one does. That round has landed and is frozen: renaming it
/// on this branch would leave the trunk's copy and a renamed one side by side.
/// A trunk directory of the same name with other contents is a different round
/// that took the id first, which is the case a rename is for.
pub(crate) fn landed_on(cfg: &Config, old: &str) -> Option<&'static str> {
    let rel = pathdiff(&cfg.repo_root, &design_rounds_dir(cfg));
    let here = git_line(cfg, &["rev-parse", &format!("HEAD:{rel}/{old}")])?;
    ["origin/dev", "origin/main"].into_iter().find(|trunk| {
        git_line(cfg, &["rev-parse", &format!("{trunk}:{rel}/{old}")]).is_some_and(|t| t == here)
    })
}

/// `cargo mock round-id`: print a fresh id, and nothing else, on stdout.
pub fn cmd_round_id(cfg: &Config) -> ExitCode {
    let Some(stamp) = stamp_now() else {
        eprintln!("error: could not read the clock with `date +%Y%m%d%H%M`");
        return ExitCode::FAILURE;
    };
    let taken = taken_ids(cfg);
    println!("{}", round_id::fresh(&stamp, &seed(cfg), |c| taken.contains(c)));
    ExitCode::SUCCESS
}

/// A round renamed: every path that moved, and the names before and after.
#[derive(Debug)]
pub(crate) struct Renamed {
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    /// `(from, to)` for every file that moved.
    pub(crate) moves:    Vec<(PathBuf, PathBuf)>,
    /// Files rewritten in place at their new path.
    pub(crate) rewrote:  Vec<PathBuf>,
}

/// `.meta` with its `round:` line pointing at `new_dir`. Every other line stays.
pub(crate) fn rewrite_meta(text: &str, old_dir: &str, new_dir: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let body = line.trim_end_matches(['\r', '\n']);
            if body == format!("round: {old_dir}") {
                format!("round: {new_dir}{}", &line[body.len() ..])
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// Rename the round `old` (a closed directory's name, or the id of the active
/// round's flat files) to `new_id`, on disk. A directory named `old` wins over
/// flat files that carry the same id.
pub(crate) fn rename_round_on_disk(dr: &Path, old: &str, new_id: &str) -> Result<Renamed, String> {
    if !matches!(round_id::split_prefix(new_id), Some((id, "")) if id == new_id) {
        return Err(format!(
            "`{new_id}` is not a round id (YYYYMMDDHHMM, or YYYYMMDDHHMM-tag)"
        ));
    }
    let base = round_id::base_id(old)
        .ok_or_else(|| format!("`{old}` does not start with a round id (YYYYMMDDHHMM)"))?;
    let dir = dr.join(old);
    let mut moves = Vec::new();
    let mut rewrote = Vec::new();

    if dir.is_dir() {
        let new_name = format!("{new_id}{}", round_id::kept_tail(old));
        let new_dir = dr.join(&new_name);
        if new_dir.exists() {
            return Err(format!("`{new_name}` already exists under design_rounds/"));
        }
        // every file in the directory moves with it, renamed where it carries the id
        let renames: Vec<(String, String)> = names_in(&dir)
            .into_iter()
            .map(|n| {
                let to = round_id::rename_prefix(&n, base, new_id).unwrap_or_else(|| n.clone());
                (n, to)
            })
            .collect();
        for (from, to) in renames.iter().filter(|(from, to)| from != to) {
            fs::rename(dir.join(from), dir.join(to)).map_err(|e| format!("rename {from}: {e}"))?;
        }
        fs::rename(&dir, &new_dir).map_err(|e| format!("rename {old}/: {e}"))?;
        for (from, to) in renames {
            moves.push((dr.join(old).join(&from), new_dir.join(&to)));
        }
        let meta = new_dir.join(".meta");
        if let Ok(text) = fs::read_to_string(&meta) {
            let next = rewrite_meta(&text, old, &new_name);
            if next != text {
                fs::write(&meta, next).map_err(|e| format!("write .meta: {e}"))?;
                rewrote.push(meta);
            }
        }
        return Ok(Renamed { old_name: old.to_string(), new_name, moves, rewrote });
    }

    let renames: Vec<(String, String)> = names_in(dr)
        .into_iter()
        .filter_map(|n| round_id::rename_prefix(&n, old, new_id).map(|m| (n, m)))
        .collect();
    if renames.is_empty() {
        return Err(format!(
            "no round `{old}`: neither a directory nor flat files under design_rounds/"
        ));
    }
    if let Some((_, to)) = renames.iter().find(|(_, to)| dr.join(to).exists()) {
        return Err(format!("`{to}` already exists under design_rounds/"));
    }
    for (from, to) in renames {
        fs::rename(dr.join(&from), dr.join(&to)).map_err(|e| format!("rename {from}: {e}"))?;
        moves.push((dr.join(from), dr.join(to)));
    }
    Ok(Renamed { old_name: old.to_string(), new_name: new_id.to_string(), moves, rewrote })
}

/// Move the local `round/<old>/*` tags to `round/<new>/*`, keeping each tag's
/// object. Returns what moved, as `(old tag, new tag)`.
fn move_tags(cfg: &Config, old: &str, new: &str) -> Vec<(String, String)> {
    let Some(listing) = git_line(cfg, &["tag", "-l", &format!("round/{old}/*")]) else {
        return Vec::new();
    };
    let mut moved = Vec::new();
    for tag in listing.lines() {
        let new_tag = tag.replacen(&format!("round/{old}/"), &format!("round/{new}/"), 1);
        let Some(object) = git_line(cfg, &["rev-parse", &format!("refs/tags/{tag}")]) else {
            continue;
        };
        let made = Command::new("git")
            .args(["update-ref", &format!("refs/tags/{new_tag}"), &object])
            .current_dir(&cfg.repo_root)
            .status()
            .is_ok_and(|s| s.success());
        if made && git_line(cfg, &["tag", "-d", tag]).is_some() {
            moved.push((tag.to_string(), new_tag));
        }
    }
    moved
}

/// `cargo mock rename-round <old> [<new>]`.
///
/// Renames the round `old` to `new`, or to a fresh id on the same minute when
/// `new` is left out: the directory, every file inside that starts with the old
/// id, the `round:` line of `.meta`, and the local `round/<old>/*` tags.
pub fn cmd_rename_round(cfg: &Config, opts: &SubcmdOpts, args: &[&str]) -> ExitCode {
    let positional: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    let (old, wanted) = match positional[..] {
        [old] => (old, None),
        [old, new] => (old, Some(new)),
        _ => {
            eprintln!("usage: cargo mock rename-round <old id or directory> [<new id>]");
            return ExitCode::FAILURE;
        },
    };
    let dr = design_rounds_dir(cfg);
    let Some(base) = round_id::base_id(old) else {
        eprintln!("error: `{old}` does not start with a round id (YYYYMMDDHHMM)");
        return ExitCode::FAILURE;
    };
    if let Some(trunk) = landed_on(cfg, old) {
        eprintln!(
            "error: round `{old}` is already on {trunk} exactly as it is here, so it has landed \
             and its id is frozen; rename the round of that id that this branch closed, which \
             differs from the one on {trunk}"
        );
        return ExitCode::FAILURE;
    }
    let taken = taken_ids(cfg);
    let new_id = match wanted {
        Some(new) if taken.contains(new) => {
            eprintln!("error: round id `{new}` is already in use here");
            return ExitCode::FAILURE;
        },
        Some(new) => new.to_string(),
        None => round_id::fresh(&base[.. 12], &seed(cfg), |c| taken.contains(c)),
    };
    let renamed = match rename_round_on_disk(&dr, old, &new_id) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    let tags = move_tags(cfg, &renamed.old_name, &renamed.new_name);
    eprintln!("renamed round {} -> {}", renamed.old_name, renamed.new_name);
    for (old_tag, new_tag) in &tags {
        eprintln!("  tag {old_tag} -> {new_tag}");
    }
    let mut touched: Vec<PathBuf> = renamed
        .moves
        .iter()
        .flat_map(|(from, to)| [from.clone(), to.clone()])
        .collect();
    touched.extend(renamed.rewrote);
    let msg = super::subjects::rename_round(&renamed.old_name, &renamed.new_name);
    commit_or_suggest(cfg, opts, &touched, &msg);
    ExitCode::SUCCESS
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
