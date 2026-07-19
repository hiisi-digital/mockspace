#![allow(unused_imports)]
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

/// Move every non-README file under `dr` into a `<archive_dir_name>/`
/// subdirectory and emit `.meta` + `.history` metadata. Stages the moves
/// for commit (or prints the manual command if `auto_commit` is unset)
/// and tags `round/<archive_dir_name>/<suffix>` on success.
pub(crate) fn perform_archive(
    cfg: &Config,
    opts: &SubcmdOpts,
    dr: &Path,
    archive_dir_name: &str,
    kind: ArchiveKind,
) -> ExitCode {
    // Disambiguate a colliding archive dir. The round-id is the earliest
    // changelist's minute-resolution timestamp (determine_round_name), so two
    // rounds whose first changelist landed in the same minute (across sessions)
    // would compute the same name and the second close would otherwise hard-error
    // with a complete-but-unarchivable round. Append `-2`, `-3`, ... until the
    // name is free so a finished round always archives; the disambiguated name
    // flows into the dir, `.meta` round id, commit subject, and tag uniformly.
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

    let entries: Vec<_> = fs::read_dir(dr)
        .expect("can't read design_rounds")
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();

    let mut touched = Vec::new();
    let mut moved = 0u32;
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "README.md" {
            continue;
        }
        let old_path = entry.path();
        let dest = archive_dir.join(&name);
        touched.push(old_path.clone());
        fs::rename(&old_path, &dest).unwrap_or_else(|e| panic!("failed to move {name}: {e}"));
        touched.push(dest);
        moved += 1;
    }

    eprintln!("moved {moved} files to {archive_dir_name}/");

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
        if let Some(prefix) = name.get(.. 12) {
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                prefixes.push(prefix.to_string());
            }
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
    if first.len() >= 12 && first[.. 12].chars().all(|c| c.is_ascii_digit()) {
        return first[.. 12].to_string();
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
