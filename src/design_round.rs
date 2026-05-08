//! Subcommands for managing design round lifecycle.
//!
//! `cargo mock lock` — lock the current phase's changelist.
//! `cargo mock deprecate` — deprecate the current unlocked changelist.
//! `cargo mock unlock` — destructive: nuke source, deprecate src CL, unlock doc CL.
//! `cargo mock close` — archive a completed round (CLOSED phase).
//! `cargo mock archive` — archive an abandoned round from any phase.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::changelist_helpers::{
    self, ClKind, ClStatus, Phase, ParsedChangelist,
};

use crate::config::Config;

/// Options parsed from CLI flags for design round subcommands.
pub struct SubcmdOpts {
    pub auto_commit: bool,
}

/// Resolve the design_rounds directory from config.
fn design_rounds_dir(cfg: &Config) -> PathBuf {
    cfg.mock_dir.join("design_rounds")
}

/// Result of a changelist rename: old and new absolute paths + the new filename.
struct RenameResult {
    old_path: PathBuf,
    new_path: PathBuf,
    new_name: String,
}

/// Rename a changelist file by replacing its status suffix.
fn rename_cl(dir: &Path, cl: &ParsedChangelist, new_status: ClStatus) -> Result<RenameResult, String> {
    let old_path = dir.join(&cl.filename);
    let new_name = rewrite_filename(&cl.filename, cl.kind, new_status)
        .ok_or_else(|| format!("cannot compute new filename for {}", cl.filename))?;
    let new_path = dir.join(&new_name);

    fs::rename(&old_path, &new_path)
        .map_err(|e| format!("rename {} → {}: {e}", cl.filename, new_name))?;

    Ok(RenameResult { old_path, new_path, new_name })
}

/// Rewrite a changelist filename to use a different status suffix.
fn rewrite_filename(name: &str, kind: ClKind, new_status: ClStatus) -> Option<String> {
    let kind_str = match kind {
        ClKind::Doc => "doc",
        ClKind::Src => "src",
    };
    let status_suffix = match new_status {
        ClStatus::Active => "md",
        ClStatus::Locked => "lock.md",
        ClStatus::Deprecated => "deprecated.md",
    };

    // Format: {YYYYMMDDHHMM}_changelist.{kind}.{status}.md
    if name.len() >= 12 && name[..12].chars().all(|c| c.is_ascii_digit()) {
        if let Some(pos) = name.find("_changelist.") {
            let prefix = &name[..pos];
            return Some(format!("{prefix}_changelist.{kind_str}.{status_suffix}"));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// lock
// ---------------------------------------------------------------------------

pub fn cmd_lock(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active doc changelist found");
                    return ExitCode::FAILURE;
                }
            };
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked doc changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: DOC → DRAFT");
                    eprintln!("  next: create a src changelist, then `cargo mock lock` again");
                    let msg = format!("chore: lock doc changelist for {}", r.new_name);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                }
                Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
            }
        }
        Phase::Src => {
            let cl = match changelist_helpers::find_active_src_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active src changelist found");
                    return ExitCode::FAILURE;
                }
            };
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked src changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: IMPL → CLOSED");
                    eprintln!("  next: `cargo mock close` to archive the round");
                    let msg = format!("chore: lock src changelist for {}", r.new_name);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                }
                Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
            }
        }
        Phase::Topic => {
            eprintln!("error: no changelist to lock (TOPIC phase)");
            eprintln!("  create a doc changelist first");
            ExitCode::FAILURE
        }
        Phase::SrcPlan => {
            eprintln!("error: doc CL already locked, no src CL to lock (DRAFT phase)");
            eprintln!("  create a src changelist first");
            ExitCode::FAILURE
        }
        Phase::Done => {
            eprintln!("error: both changelists already locked (CLOSED phase)");
            eprintln!("  use `cargo mock close` to archive the round");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// deprecate
// ---------------------------------------------------------------------------

pub fn cmd_deprecate(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active doc changelist found");
                    return ExitCode::FAILURE;
                }
            };
            match rename_cl(&dr, &cl, ClStatus::Deprecated) {
                Ok(r) => {
                    eprintln!("deprecated doc changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: DOC → TOPIC");
                    eprintln!("  next: create new topic files, then a new changelist");
                    let msg = format!("chore: deprecate doc changelist {}", cl.filename);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                }
                Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
            }
        }
        Phase::Src => {
            let cl = match changelist_helpers::find_active_src_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active src changelist found");
                    return ExitCode::FAILURE;
                }
            };

            let mut touched = Vec::new();

            // Step 1: deprecate the src CL
            match rename_cl(&dr, &cl, ClStatus::Deprecated) {
                Ok(r) => {
                    eprintln!("deprecated src changelist: {} → {}", cl.filename, r.new_name);
                    touched.push(r.old_path);
                    touched.push(r.new_path);
                }
                Err(e) => { eprintln!("error: {e}"); return ExitCode::FAILURE; }
            }

            // Step 2: unlock the doc CL (DRAFT is a useless intermediate state)
            if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&dr) {
                match rename_cl(&dr, &doc_cl, ClStatus::Active) {
                    Ok(r) => {
                        eprintln!("unlocked doc changelist: {} → {}", doc_cl.filename, r.new_name);
                        touched.push(r.old_path);
                        touched.push(r.new_path);
                    }
                    Err(e) => { eprintln!("error: {e}"); return ExitCode::FAILURE; }
                }
            }

            eprintln!("  phase transition: IMPL → DOC");
            eprintln!("  next: update doc templates, then lock and create new src changelist");
            let msg = format!("chore: deprecate src changelist {} and unlock doc CL", cl.filename);
            commit_or_suggest(cfg, opts, &touched, &msg);
            ExitCode::SUCCESS
        }
        Phase::Topic => {
            eprintln!("error: no changelist to deprecate (TOPIC phase)");
            ExitCode::FAILURE
        }
        Phase::SrcPlan => {
            eprintln!("error: doc CL is locked (DRAFT phase)");
            eprintln!("  use `cargo mock unlock` to unlock it first");
            ExitCode::FAILURE
        }
        Phase::Done => {
            eprintln!("error: both CLs locked (CLOSED phase)");
            eprintln!("  use `cargo mock unlock` to unlock the src CL first");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// unlock
// ---------------------------------------------------------------------------

pub fn cmd_unlock(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    match phase {
        Phase::SrcPlan | Phase::Src | Phase::Done => {}
        _ => {
            eprintln!("error: unlock requires a locked doc CL (current phase: {})", phase.label());
            eprintln!("  unlock is only available in DRAFT, IMPL, or CLOSED phases");
            return ExitCode::FAILURE;
        }
    }

    eprintln!("WARNING: `unlock` is destructive.");
    eprintln!("  it will deprecate the src CL (if any) and unlock the doc CL.");
    eprintln!("  source changes made during IMPL phase are NOT automatically reverted.");
    eprintln!("  you must manually revert source changes if needed.");
    eprintln!();

    let mut touched = Vec::new();

    // Step 1: deprecate active or locked src CL if it exists.
    if let Some(src_cl) = changelist_helpers::find_active_src_cl(&dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
                touched.push(r.old_path);
                touched.push(r.new_path);
            }
            Err(e) => { eprintln!("error: {e}"); return ExitCode::FAILURE; }
        }
    }
    if let Some(src_cl) = changelist_helpers::find_locked_src_cl(&dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
                touched.push(r.old_path);
                touched.push(r.new_path);
            }
            Err(e) => { eprintln!("error: {e}"); return ExitCode::FAILURE; }
        }
    }

    // Step 2: unlock doc CL (rename *.lock.md → *.md).
    if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&dr) {
        match rename_cl(&dr, &doc_cl, ClStatus::Active) {
            Ok(r) => {
                eprintln!("  unlocked doc CL: {} → {}", doc_cl.filename, r.new_name);
                touched.push(r.old_path);
                touched.push(r.new_path);
            }
            Err(e) => { eprintln!("error: {e}"); return ExitCode::FAILURE; }
        }
    }

    eprintln!();
    eprintln!("  phase transition: {} → DOC", phase.label());
    let msg = "chore: unlock design round (deprecate src CL, unlock doc CL)";
    commit_or_suggest(cfg, opts, &touched, msg);
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// close
// ---------------------------------------------------------------------------

pub fn cmd_close(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    if phase != Phase::Done {
        eprintln!("error: can only close a round in CLOSED phase (current: {})", phase.label());
        eprintln!("  both doc and src changelists must be locked");
        eprintln!("  for an abandoned round, use `cargo mock archive` instead");
        return ExitCode::FAILURE;
    }

    let all_cls = changelist_helpers::find_changelists(&dr);
    let round_name = determine_round_name(&all_cls);

    perform_archive(
        cfg,
        opts,
        &dr,
        &round_name,
        ArchiveKind::Closed,
    )
}

pub fn cmd_archive(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    if !dr.is_dir() {
        eprintln!("error: design_rounds/ directory not found");
        return ExitCode::FAILURE;
    }

    let round_name = match determine_round_name_from_dir(&dr) {
        Some(name) => name,
        None => {
            eprintln!("error: no round files to archive (design_rounds/ has no timestamp-prefixed files)");
            return ExitCode::FAILURE;
        }
    };

    perform_archive(
        cfg,
        opts,
        &dr,
        &format!("{round_name}-abandoned"),
        ArchiveKind::Abandoned,
    )
}

#[derive(Copy, Clone)]
enum ArchiveKind {
    Closed,
    Abandoned,
}

impl ArchiveKind {
    fn meta_status_line(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "abandoned: false",
            ArchiveKind::Abandoned => "abandoned: true",
        }
    }
    fn tag_suffix(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "end",
            ArchiveKind::Abandoned => "abandoned",
        }
    }
    fn commit_subject(self, archive_dir_name: &str) -> String {
        match self {
            ArchiveKind::Closed =>
                format!("chore: close design round {archive_dir_name}"),
            ArchiveKind::Abandoned =>
                format!("chore: archive design round {archive_dir_name} (abandoned)"),
        }
    }
    fn announce_verb(self) -> &'static str {
        match self {
            ArchiveKind::Closed => "round closed",
            ArchiveKind::Abandoned => "round archived (abandoned)",
        }
    }
}

/// Move every non-README file under `dr` into a `<archive_dir_name>/`
/// subdirectory and emit `.meta` + `.history` metadata. Stages the moves
/// for commit (or prints the manual command if `auto_commit` is unset)
/// and tags `round/<archive_dir_name>/<suffix>` on success.
fn perform_archive(
    cfg: &Config,
    opts: &SubcmdOpts,
    dr: &Path,
    archive_dir_name: &str,
    kind: ArchiveKind,
) -> ExitCode {
    let archive_dir = dr.join(archive_dir_name);
    if archive_dir.exists() {
        eprintln!("error: archive directory already exists: {}", archive_dir.display());
        return ExitCode::FAILURE;
    }

    fs::create_dir_all(&archive_dir)
        .expect("failed to create archive directory");

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
        fs::rename(&old_path, &dest)
            .unwrap_or_else(|e| panic!("failed to move {name}: {e}"));
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
fn determine_round_name_from_dir(dr: &Path) -> Option<String> {
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
        if let Some(prefix) = name.get(..12) {
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
fn determine_round_name(cls: &[ParsedChangelist]) -> String {
    // Prefer non-deprecated changelists for naming.
    let relevant: Vec<&ParsedChangelist> = cls.iter()
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
    if first.len() >= 12 && first[..12].chars().all(|c| c.is_ascii_digit()) {
        return first[..12].to_string();
    }
    // Legacy: YYYY-MM-DD
    if first.len() >= 10 {
        return first[..10].to_string();
    }

    "unknown-round".to_string()
}

// ---------------------------------------------------------------------------
// migrate
// ---------------------------------------------------------------------------

pub fn cmd_migrate(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);

    if !dr.is_dir() {
        eprintln!("error: design_rounds/ directory not found");
        return ExitCode::FAILURE;
    }

    let entries: Vec<_> = fs::read_dir(&dr)
        .expect("can't read design_rounds")
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();

    let mut touched = Vec::new();
    let mut renamed = 0u32;
    let mut skipped = 0u32;

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip README.md and non-.md files.
        if name == "README.md" || !name.ends_with(".md") {
            continue;
        }

        // Skip files already in new format (12-digit prefix).
        if !is_legacy_filename(&name) {
            skipped += 1;
            continue;
        }

        let new_name = match legacy_to_new_filename(&name) {
            Some(n) => n,
            None => {
                eprintln!("  skip (unrecognized): {name}");
                skipped += 1;
                continue;
            }
        };

        let old_path = entry.path();
        let new_path = dr.join(&new_name);

        if new_path.exists() {
            eprintln!("  skip (target exists): {name} → {new_name}");
            skipped += 1;
            continue;
        }

        fs::rename(&old_path, &new_path)
            .unwrap_or_else(|e| panic!("failed to rename {name} → {new_name}: {e}"));

        eprintln!("  {name} → {new_name}");
        touched.push(old_path);
        touched.push(new_path);
        renamed += 1;
    }

    if renamed == 0 {
        eprintln!("nothing to migrate ({skipped} files already in new format or skipped)");
        return ExitCode::SUCCESS;
    }

    eprintln!("migrated {renamed} file(s), skipped {skipped}");
    let msg = format!("chore: migrate {renamed} design round file(s) to new naming convention");
    commit_or_suggest(cfg, opts, &touched, &msg);
    ExitCode::SUCCESS
}

/// Returns true if the filename uses legacy `YYYY-MM-DD_` prefix format.
fn is_legacy_filename(name: &str) -> bool {
    // Legacy: YYYY-MM-DD_ (11 chars: 4 digits, dash, 2 digits, dash, 2 digits, underscore)
    if name.len() < 11 {
        return false;
    }
    let bytes = name.as_bytes();
    bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        && bytes[10] == b'_'
}

/// Convert a legacy filename to the new naming convention.
///
/// Topics: `2026-03-07_corrections.md` → `202603070000_topic.corrections.md`
/// Changelists: `2026-03-07_changelist.md` → `202603070000_changelist.doc.md`
/// Changelists: `2026-03-07_changelist.lock.md` → `202603070000_changelist.doc.lock.md`
/// Changelists: `2026-03-07_foo_changelist.md` → `202603070000_changelist.doc.md`
fn legacy_to_new_filename(name: &str) -> Option<String> {
    if !is_legacy_filename(name) {
        return None;
    }

    // Extract date parts and convert to compact timestamp.
    let year = &name[0..4];
    let month = &name[5..7];
    let day = &name[8..10];
    let timestamp = format!("{year}{month}{day}0000");

    // Everything after the date prefix (YYYY-MM-DD_).
    let rest = &name[11..];

    // Determine if it's a changelist.
    if let Some(cl_pos) = rest.find("changelist") {
        // It's a changelist. Determine the status suffix.
        let after_cl = &rest[cl_pos + "changelist".len()..];
        let status_suffix = if after_cl.starts_with(".lock.md") {
            "lock.md"
        } else if after_cl.starts_with(".deprecated.md") {
            "deprecated.md"
        } else if after_cl.starts_with(".md") {
            "md"
        } else {
            return None;
        };
        return Some(format!("{timestamp}_changelist.doc.{status_suffix}"));
    }

    // It's a topic. Extract name (strip .md suffix).
    let topic_name = rest.strip_suffix(".md")?;
    if topic_name.is_empty() {
        return None;
    }
    Some(format!("{timestamp}_topic.{topic_name}.md"))
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Either surgically commit the exact files we touched, or print a suggestion.
///
/// When `opts.auto_commit` is true, uses `git commit --only -- <paths>`
/// to commit ONLY the specified files. This ignores any previously staged
/// changes (they remain staged after the commit) and any other unstaged
/// modifications. No stash needed.
///
/// `--only` works by:
///   1. Temporarily resetting the index for the named paths
///   2. Staging their current working-tree state (including deletions)
///   3. Creating the commit from that temporary index
///   4. Merging the temporary index back with the original
///
/// The result: only our files are committed; everything else is untouched.
fn commit_or_suggest(cfg: &Config, opts: &SubcmdOpts, files: &[PathBuf], message: &str) {
    if !opts.auto_commit {
        let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
        eprintln!();
        eprintln!("  to commit:");
        eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
        return;
    }

    let root = &cfg.repo_root;

    // Use a temporary index to create a surgical commit.
    // This never touches the real index, so all existing staged changes
    // are preserved exactly as they were.
    //
    // Steps:
    //   1. Create temp index from HEAD tree
    //   2. Update temp index with our files (adds + removes)
    //   3. Write tree from temp index
    //   4. Create commit object pointing to that tree
    //   5. Update HEAD ref
    //   6. Clean up temp index
    //
    // The real .git/index is never read or modified.
    let git_dir = root.join(".git");
    let tmp_index = git_dir.join("tmp_mockspace_index");

    // Clean up any stale temp index.
    let _ = fs::remove_file(&tmp_index);

    let tmp_idx_str = tmp_index.to_string_lossy().to_string();

    // 1. Populate temp index from HEAD.
    if git_env_run(root, &["read-tree", "HEAD"], &tmp_idx_str).is_err() {
        eprintln!("error: git read-tree HEAD failed");
        suggest_fallback(cfg, message);
        return;
    }

    // 2. Update temp index with our changes.
    for f in files {
        let rel = pathdiff(root, f);
        if f.exists() {
            // Add or update file in temp index.
            if git_env_run(root, &["update-index", "--add", &rel], &tmp_idx_str).is_err() {
                eprintln!("warning: failed to add {rel} to temp index");
            }
        } else {
            // Remove deleted file from temp index (ignore if not present).
            let _ = git_env_run(root, &["update-index", "--remove", &rel], &tmp_idx_str);
        }
    }

    // 3. Write tree.
    let tree_sha = match git_env_ok(root, &["write-tree"], &tmp_idx_str) {
        Ok(sha) => sha.trim().to_string(),
        Err(e) => {
            eprintln!("error: git write-tree failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 4. Get current HEAD sha.
    let head_sha = match git_ok(root, &["rev-parse", "HEAD"]) {
        Ok(sha) => sha.trim().to_string(),
        Err(e) => {
            eprintln!("error: git rev-parse HEAD failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 5. Create commit object.
    let commit_sha = match Command::new("git")
        .args(["commit-tree", &tree_sha, "-p", &head_sha, "-m", message])
        .current_dir(root)
        .output()
    {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        Ok(o) => {
            eprintln!("error: git commit-tree failed: {}", String::from_utf8_lossy(&o.stderr));
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
        Err(e) => {
            eprintln!("error: git commit-tree failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 6. Update HEAD to point to new commit.
    let branch = git_ok(root, &["symbolic-ref", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "HEAD".to_string());

    let ref_name = if branch == "HEAD" {
        "HEAD".to_string()
    } else {
        format!("refs/heads/{branch}")
    };

    if git_run(root, &["update-ref", &ref_name, &commit_sha]).is_err() {
        eprintln!("error: git update-ref failed");
        eprintln!("  commit object created: {commit_sha}");
        eprintln!("  run: git update-ref {ref_name} {commit_sha}");
    } else {
        eprintln!("  committed: {message}");
    }

    // 7. Clean up.
    let _ = fs::remove_file(&tmp_index);
}

/// Print fallback manual commit instructions.
fn suggest_fallback(cfg: &Config, message: &str) {
    let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
    eprintln!("  to commit manually:");
    eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
}

/// Run a git command and return Ok(stdout) or Err(stderr).
fn git_ok(root: &Path, args: &[&str]) -> Result<String, String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).map_err(|e| e.to_string())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).to_string())
            }
        })
}

/// Run a git command; returns Ok(()) on success, Err(stderr) on failure.
fn git_run(root: &Path, args: &[&str]) -> Result<(), String> {
    git_ok(root, args).map(|_| ())
}

/// Run a git command with a custom GIT_INDEX_FILE; returns Ok(stdout).
fn git_env_ok(root: &Path, args: &[&str], index_file: &str) -> Result<String, String> {
    Command::new("git")
        .args(args)
        .env("GIT_INDEX_FILE", index_file)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).map_err(|e| e.to_string())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).to_string())
            }
        })
}

/// Run a git command with a custom GIT_INDEX_FILE; returns Ok(()).
fn git_env_run(root: &Path, args: &[&str], index_file: &str) -> Result<(), String> {
    git_env_ok(root, args, index_file).map(|_| ())
}

/// Get a relative path from base to target.
fn pathdiff(base: &Path, target: &Path) -> String {
    target.strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target.to_string_lossy().to_string())
}

fn git_head_sha(cfg: &Config) -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&cfg.repo_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn git_round_log(cfg: &Config) -> String {
    // Get log since the round/*/start tag, or last 50 commits.
    Command::new("git")
        .args(["log", "--oneline", "-50"])
        .current_dir(&cfg.repo_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

fn chrono_date() -> String {
    // Simple date without chrono dependency.
    Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_new_format_doc_to_locked() {
        let result = rewrite_filename("202603071430_changelist.doc.md", ClKind::Doc, ClStatus::Locked);
        assert_eq!(result.unwrap(), "202603071430_changelist.doc.lock.md");
    }

    #[test]
    fn rewrite_new_format_src_to_deprecated() {
        let result = rewrite_filename("202603071430_changelist.src.md", ClKind::Src, ClStatus::Deprecated);
        assert_eq!(result.unwrap(), "202603071430_changelist.src.deprecated.md");
    }

    #[test]
    fn rewrite_locked_to_active() {
        let result = rewrite_filename("202603071430_changelist.doc.lock.md", ClKind::Doc, ClStatus::Active);
        assert_eq!(result.unwrap(), "202603071430_changelist.doc.md");
    }

    #[test]
    fn round_name_from_new_format() {
        let cls = vec![
            ParsedChangelist {
                filename: "202603101430_changelist.doc.lock.md".to_string(),
                kind: ClKind::Doc,
                status: ClStatus::Locked,
            },
            ParsedChangelist {
                filename: "202603101500_changelist.src.lock.md".to_string(),
                kind: ClKind::Src,
                status: ClStatus::Locked,
            },
        ];
        assert_eq!(determine_round_name(&cls), "202603101430");
    }

    #[test]
    fn round_name_from_legacy() {
        let cls = vec![
            ParsedChangelist {
                filename: "2026-03-07_changelist.lock.md".to_string(),
                kind: ClKind::Doc,
                status: ClStatus::Locked,
            },
        ];
        assert_eq!(determine_round_name(&cls), "2026-03-07");
    }

    // --- migrate tests ---

    #[test]
    fn detect_legacy_filename() {
        assert!(is_legacy_filename("2026-03-07_corrections.md"));
        assert!(is_legacy_filename("2026-03-06_source-doc-divergence-audit.md"));
        assert!(is_legacy_filename("2026-03-07_changelist.md"));
        assert!(is_legacy_filename("2026-03-07_changelist.lock.md"));
        assert!(!is_legacy_filename("202603070000_topic.corrections.md"));
        assert!(!is_legacy_filename("README.md"));
        assert!(!is_legacy_filename("short.md"));
    }

    #[test]
    fn migrate_topic_simple() {
        let result = legacy_to_new_filename("2026-03-07_corrections.md");
        assert_eq!(result.unwrap(), "202603070000_topic.corrections.md");
    }

    #[test]
    fn migrate_topic_hyphenated() {
        let result = legacy_to_new_filename("2026-03-06_source-doc-divergence-audit.md");
        assert_eq!(result.unwrap(), "202603060000_topic.source-doc-divergence-audit.md");
    }

    #[test]
    fn migrate_topic_string_primitive() {
        let result = legacy_to_new_filename("2026-03-07_string-primitive.md");
        assert_eq!(result.unwrap(), "202603070000_topic.string-primitive.md");
    }

    #[test]
    fn migrate_changelist_active() {
        let result = legacy_to_new_filename("2026-03-07_changelist.md");
        assert_eq!(result.unwrap(), "202603070000_changelist.doc.md");
    }

    #[test]
    fn migrate_changelist_locked() {
        let result = legacy_to_new_filename("2026-03-07_changelist.lock.md");
        assert_eq!(result.unwrap(), "202603070000_changelist.doc.lock.md");
    }

    #[test]
    fn migrate_changelist_deprecated() {
        let result = legacy_to_new_filename("2026-03-07_changelist.deprecated.md");
        assert_eq!(result.unwrap(), "202603070000_changelist.doc.deprecated.md");
    }

    #[test]
    fn migrate_changelist_with_name_prefix() {
        let result = legacy_to_new_filename("2026-03-07_foo_changelist.md");
        assert_eq!(result.unwrap(), "202603070000_changelist.doc.md");
    }

    #[test]
    fn migrate_not_legacy_returns_none() {
        assert!(legacy_to_new_filename("202603070000_topic.corrections.md").is_none());
        assert!(legacy_to_new_filename("README.md").is_none());
    }

    // --- archive tests ---

    #[test]
    fn archive_round_name_picks_earliest_timestamp() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dr = tmp.path();
        // Mix CLs and topic files. The earliest 12-digit prefix wins.
        std::fs::write(dr.join("202604201200_topic.alpha.md"), "x").unwrap();
        std::fs::write(dr.join("202604191100_topic.beta.md"), "x").unwrap();
        std::fs::write(dr.join("202604221400_changelist.doc.deprecated.md"), "x").unwrap();
        std::fs::write(dr.join("README.md"), "x").unwrap();
        let name = determine_round_name_from_dir(dr).expect("found a name");
        assert_eq!(name, "202604191100");
    }

    #[test]
    fn archive_round_name_topic_only() {
        // TOPIC-phase abandonment with no changelist files at all.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dr = tmp.path();
        std::fs::write(dr.join("202604211500_topic.gamma.md"), "x").unwrap();
        let name = determine_round_name_from_dir(dr).expect("found a name");
        assert_eq!(name, "202604211500");
    }

    #[test]
    fn archive_round_name_skips_non_timestamp_files() {
        // Anything without a 12-digit prefix is ignored. README, leftover
        // notes, dotfiles produced elsewhere — none should affect naming.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dr = tmp.path();
        std::fs::write(dr.join("README.md"), "x").unwrap();
        std::fs::write(dr.join("notes.md"), "x").unwrap();
        std::fs::write(dr.join(".gitignore"), "x").unwrap();
        std::fs::write(dr.join("202604221600_topic.delta.md"), "x").unwrap();
        let name = determine_round_name_from_dir(dr).expect("found a name");
        assert_eq!(name, "202604221600");
    }

    #[test]
    fn archive_round_name_empty_dir_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("README.md"), "x").unwrap();
        assert!(determine_round_name_from_dir(tmp.path()).is_none());
    }

    #[test]
    fn archive_kind_meta_and_tag_strings() {
        // Lock down the strings emitted into .meta and tag names so a
        // consumer reading archive metadata can't be tripped by silent
        // changes.
        assert_eq!(ArchiveKind::Closed.meta_status_line(), "abandoned: false");
        assert_eq!(ArchiveKind::Abandoned.meta_status_line(), "abandoned: true");
        assert_eq!(ArchiveKind::Closed.tag_suffix(), "end");
        assert_eq!(ArchiveKind::Abandoned.tag_suffix(), "abandoned");
        assert_eq!(
            ArchiveKind::Closed.commit_subject("202604191100"),
            "chore: close design round 202604191100",
        );
        assert_eq!(
            ArchiveKind::Abandoned.commit_subject("202604191100-abandoned"),
            "chore: archive design round 202604191100-abandoned (abandoned)",
        );
    }
}
