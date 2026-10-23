//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

#[derive(Copy, Clone)]
pub(crate) enum ArchiveKind {
    Closed,
    Abandoned,
}

impl ArchiveKind {
    pub(crate) fn meta_status_line(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "abandoned: false",
            ArchiveKind::Abandoned => "abandoned: true",
        }
    }

    pub(crate) fn tag_suffix(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "end",
            ArchiveKind::Abandoned => "abandoned",
        }
    }

    pub(crate) fn commit_subject(self, archive_dir_name: &str) -> String {
        match self {
            ArchiveKind::Closed => format!("chore: close design round {archive_dir_name}"),
            ArchiveKind::Abandoned => {
                format!("chore: archive design round {archive_dir_name} (abandoned)")
            },
        }
    }

    pub(crate) fn announce_verb(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "round closed",
            ArchiveKind::Abandoned => "round archived (abandoned)",
        }
    }
}

/// Pick a non-colliding archive directory name. Returns `base` when it is free
/// (per the `exists` predicate); otherwise appends `-2`, `-3`, ... until a free
/// name is found. Pure over the predicate so the collision walk is unit-testable
/// without touching the filesystem.
pub(crate) fn disambiguate_archive_name(base: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(base) {
        return base.to_string();
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !exists(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// The flat files of `design_rounds/` split by whether a close takes them.
pub(crate) struct RoundFiles {
    pub(crate) taken: Vec<String>,
    pub(crate) left:  Vec<String>,
}

/// Which of the flat files `names` (README excluded) belong to the round being
/// closed, and which are another's.
///
/// `design_rounds/` holds one round's changelists at a time but any number of
/// topics, because a topic is filed while another round is in flight or parked
/// for the next. Nothing in a topic's name says which round it is for: it is
/// stamped when it was written, which is before its round's changelists
/// (the usual case) or after, and a parked one is re-filed under a new stamp
/// each time. So a file is the round's when what a round is made of says so:
///
/// - it is a changelist, of any status, because the phase is read over every
///   changelist in the directory and they are the round;
/// - or it starts with the id of one of those changelists, since the doc and
///   the src changelist of a round are stamped apart and a file written beside
///   either carries its id;
/// - or one of those changelists names it, which is how a topic written earlier
///   under an id of its own is tied to the round that answers it.
///
/// Anything else is left where it is, flat and open. A topic of the round that
/// no changelist names is therefore left too, which is the visible mistake of
/// the two: it still reads as open, where a swept topic read as closed.
/// `read` returns a file's text, or `None` where it cannot be read.
pub(crate) fn files_of_round(
    names: &[String],
    read: impl Fn(&str) -> Option<String>,
) -> RoundFiles {
    let changelists: Vec<&String> = names
        .iter()
        .filter(|n| changelist_helpers::parse_changelist(n).is_some())
        .collect();
    let ids: Vec<&str> = changelists
        .iter()
        .filter_map(|n| lint_rules::round_id::split_prefix(n).map(|(id, _)| id))
        .collect();
    let text: String = changelists
        .iter()
        .filter_map(|n| read(n))
        .collect::<Vec<_>>()
        .join("\n");

    let (taken, left) = names.iter().cloned().partition(|name| {
        changelist_helpers::parse_changelist(name).is_some()
            || lint_rules::round_id::split_prefix(name)
                .is_some_and(|(id, rest)| rest.starts_with('_') && ids.contains(&id))
            || (lint_rules::round_id::split_prefix(name).is_some() && text.contains(name.as_str()))
    });
    RoundFiles {
        taken,
        left,
    }
}

/// Move the files `files_of_round` takes (every one, for an abandoned round)
/// under `dr` into a `<archive_dir_name>/` subdirectory and emit `.meta` +
/// `.history` metadata. Stages the moves
/// for commit (or prints the manual command if `auto_commit` is unset)
/// and tags `round/<archive_dir_name>/<suffix>` on success.
pub(crate) fn perform_archive(
    cfg: &Config,
    opts: &SubcmdOpts,
    dr: &Path,
    archive_dir_name: &str,
    kind: ArchiveKind,
) -> ExitCode {
    // Disambiguate a colliding archive dir within this tree. The round id is
    // the earliest changelist's id (determine_round_name); ids made by
    // `cargo mock round-id` carry a tag and do not repeat, but a bare minute
    // from an older round still can, and the second close would otherwise
    // hard-error with a complete-but-unarchivable round. Append `-2`, `-3`, ...
    // until the name is free so a finished round always archives; the
    // disambiguated name flows into the dir, `.meta` round id, commit subject,
    // and tag uniformly. This cannot see another branch: two branches that
    // closed the same name meet in `merge-round-meta`, which refuses the merge.
    let final_name = disambiguate_archive_name(archive_dir_name, |n| dr.join(n).exists());
    if final_name != archive_dir_name {
        eprintln!(
            "note: archive dir {archive_dir_name}/ already exists (round-id collision); \
             archiving as {final_name}/ instead"
        );
    }
    let archive_dir_name: &str = &final_name;
    let archive_dir = dr.join(archive_dir_name);

    fs::create_dir_all(&archive_dir).expect("failed to create archive directory");

    let flat: Vec<String> = fs::read_dir(dr)
        .expect("can't read design_rounds")
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name != "README.md")
        .collect();
    // A closed round has its changelists to tell its files by. An abandoned
    // one may have none, and what it is for is clearing the directory.
    let RoundFiles {
        taken,
        left,
    } = match kind {
        ArchiveKind::Closed => files_of_round(&flat, |name| fs::read_to_string(dr.join(name)).ok()),
        ArchiveKind::Abandoned => {
            RoundFiles {
                taken: flat,
                left:  Vec::new(),
            }
        },
    };

    let mut touched = Vec::new();
    let mut moved = 0u32;
    for name in &taken {
        let old_path = dr.join(name);
        let dest = archive_dir.join(name);
        touched.push(old_path.clone());
        fs::rename(&old_path, &dest).unwrap_or_else(|e| panic!("failed to move {name}: {e}"));
        touched.push(dest);
        moved += 1;
    }

    eprintln!("moved {moved} files to {archive_dir_name}/");
    if !left.is_empty() {
        eprintln!("left flat, not this round's (no id of its changelists, named by none of them):");
        for name in &left {
            eprintln!("  {name}");
        }
        eprintln!("  to close one with the round, name it in a changelist before the lock");
    }

    let head_sha = git_head_sha(cfg);
    let today = chrono_date();
    let meta = format!(
        "round: {archive_dir_name}\nclosed: {head_sha}\nclose_date: {today}\n{}\n",
        kind.meta_status_line(),
    );
    let meta_path = archive_dir.join(".meta");
    fs::write(&meta_path, &meta).expect("failed to write .meta");
    touched.push(meta_path);
    eprintln!("wrote .meta");

    let history = git_round_log(cfg);
    if !history.is_empty() {
        let history_path = archive_dir.join(".history");
        fs::write(&history_path, &history).expect("failed to write .history");
        touched.push(history_path);
        eprintln!("wrote .history");
    }

    eprintln!("{}: {archive_dir_name}", kind.announce_verb());
    let msg = kind.commit_subject(archive_dir_name);
    commit_or_suggest(cfg, opts, &touched, &msg);

    let tag_name = format!("round/{archive_dir_name}/{}", kind.tag_suffix());
    if opts.auto_commit {
        let tag_result = Command::new("git")
            .args(["tag", &tag_name])
            .current_dir(&cfg.repo_root)
            .status();
        match tag_result {
            Ok(s) if s.success() => eprintln!("tagged: {tag_name}"),
            _ => eprintln!("warning: failed to create tag {tag_name}"),
        }
    } else {
        eprintln!("    git tag {tag_name}");
    }

    ExitCode::SUCCESS
}

/// Earliest 12-digit timestamp prefix among files in `dr`.
///
/// Used by `cmd_archive` when no changelists may exist (TOPIC-only
/// abandonments). Skips README, subdirectories, and any file lacking
/// the canonical `YYYYMMDDHHMM_*` prefix.
pub(crate) fn determine_round_name_from_dir(dr: &Path) -> Option<String> {
    let entries = fs::read_dir(dr).ok()?;
    let mut prefixes: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "README.md" {
            continue;
        }
        if let Some((id, _)) = lint_rules::round_id::split_prefix(&name) {
            prefixes.push(id.to_string());
        }
    }
    prefixes.sort();
    prefixes.into_iter().next()
}

/// Determine a round name from the changelist filenames.
/// Uses the timestamp prefix of the earliest changelist.
pub(crate) fn determine_round_name(cls: &[ParsedChangelist]) -> String {
    // Prefer non-deprecated changelists for naming.
    let relevant: Vec<&ParsedChangelist> = cls
        .iter()
        .filter(|cl| cl.status != ClStatus::Deprecated)
        .collect();

    let source = if relevant.is_empty() { cls } else { &[] };
    let candidates: Vec<&str> = if !relevant.is_empty() {
        relevant.iter().map(|cl| cl.filename.as_str()).collect()
    } else {
        source.iter().map(|cl| cl.filename.as_str()).collect()
    };

    if candidates.is_empty() {
        return "unknown-round".to_string();
    }

    // Sort by filename to get earliest timestamp first.
    let mut sorted = candidates;
    sorted.sort();
    let first = sorted[0];

    // Extract timestamp prefix.
    if let Some((id, _)) = lint_rules::round_id::split_prefix(first) {
        return id.to_string();
    }
    // Legacy: YYYY-MM-DD
    if first.len() >= 10 {
        return first[.. 10].to_string();
    }

    "unknown-round".to_string()
}

// ---------------------------------------------------------------------------
// migrate
// ---------------------------------------------------------------------------
