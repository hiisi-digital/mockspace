#![allow(unused_imports)]
use super::*;

/// Resolve the design_rounds directory from config.
pub(crate) fn design_rounds_dir(cfg: &Config) -> PathBuf {
    cfg.mock_dir.join("design_rounds")
}

/// Result of a changelist rename: old and new absolute paths + the new filename.
pub(crate) struct RenameResult {
    pub(crate) old_path: PathBuf,
    pub(crate) new_path: PathBuf,
    pub(crate) new_name: String,
}

/// Rename a changelist file by replacing its status suffix.
pub(crate) fn rename_cl(
    dir: &Path,
    cl: &ParsedChangelist,
    new_status: ClStatus,
) -> Result<RenameResult, String> {
    let old_path = dir.join(&cl.filename);
    let new_name = rewrite_filename(&cl.filename, cl.kind, new_status)
        .ok_or_else(|| format!("cannot compute new filename for {}", cl.filename))?;
    let new_path = dir.join(&new_name);

    fs::rename(&old_path, &new_path)
        .map_err(|e| format!("rename {} → {}: {e}", cl.filename, new_name))?;

    Ok(RenameResult {
        old_path,
        new_path,
        new_name,
    })
}

/// Rewrite a changelist filename to use a different status suffix.
pub(crate) fn rewrite_filename(name: &str, kind: ClKind, new_status: ClStatus) -> Option<String> {
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
    if name.len() >= 12 && name[.. 12].chars().all(|c| c.is_ascii_digit()) {
        if let Some(pos) = name.find("_changelist.") {
            let prefix = &name[.. pos];
            return Some(format!("{prefix}_changelist.{kind_str}.{status_suffix}"));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// lock
// ---------------------------------------------------------------------------

/// Returns true if the filename uses legacy `YYYY-MM-DD_` prefix format.
pub(crate) fn is_legacy_filename(name: &str) -> bool {
    // Legacy: YYYY-MM-DD_ (11 chars: 4 digits, dash, 2 digits, dash, 2 digits, underscore)
    if name.len() < 11 {
        return false;
    }
    let bytes = name.as_bytes();
    bytes[0 .. 4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5 .. 7].iter().all(|b| b.is_ascii_digit())
        && bytes[7] == b'-'
        && bytes[8 .. 10].iter().all(|b| b.is_ascii_digit())
        && bytes[10] == b'_'
}

/// Convert a legacy filename to the new naming convention.
///
/// Topics: `2026-03-07_corrections.md` → `202603070000_topic.corrections.md`
/// Changelists: `2026-03-07_changelist.md` → `202603070000_changelist.doc.md`
/// Changelists: `2026-03-07_changelist.lock.md` → `202603070000_changelist.doc.lock.md`
/// Changelists: `2026-03-07_foo_changelist.md` → `202603070000_changelist.doc.md`
pub(crate) fn legacy_to_new_filename(name: &str) -> Option<String> {
    if !is_legacy_filename(name) {
        return None;
    }

    // Extract date parts and convert to compact timestamp.
    let year = &name[0 .. 4];
    let month = &name[5 .. 7];
    let day = &name[8 .. 10];
    let timestamp = format!("{year}{month}{day}0000");

    // Everything after the date prefix (YYYY-MM-DD_).
    let rest = &name[11 ..];

    // Determine if it's a changelist.
    if let Some(cl_pos) = rest.find("changelist") {
        // It's a changelist. Determine the status suffix.
        let after_cl = &rest[cl_pos + "changelist".len() ..];
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
