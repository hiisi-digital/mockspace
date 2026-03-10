//! Shared helpers for changelist detection and phase determination.
//!
//! Centralises the logic for parsing changelist filenames, scanning
//! the design_rounds directory, and computing the current phase.
//!
//! Naming convention:
//! `YYYYMMDDHHMM_changelist.{doc|src}.{lock|deprecated}?.md`

use std::path::Path;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClKind {
    Doc,
    Src,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClStatus {
    Active,
    Locked,
    Deprecated,
}

#[derive(Debug, Clone)]
pub struct ParsedChangelist {
    pub filename: String,
    pub kind: ClKind,
    pub status: ClStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// No changelists at all — only topic files allowed.
    Topic,
    /// Unlocked doc CL exists — doc templates editable.
    Doc,
    /// Doc CL locked, no src CL — src CL creation only.
    SrcPlan,
    /// Doc CL locked, unlocked src CL exists — source editable.
    Src,
    /// Both CLs locked — round complete, nothing editable.
    Done,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Topic => "TOPIC",
            Phase::Doc => "DOC",
            Phase::SrcPlan => "SRC-PLAN",
            Phase::Src => "SRC",
            Phase::Done => "DONE",
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a filename into a changelist descriptor, if it matches.
///
/// Format: `YYYYMMDDHHMM_changelist.{doc|src}.{lock|deprecated}?.md`
pub fn parse_changelist(name: &str) -> Option<ParsedChangelist> {
    // Pattern: {YYYYMMDDHHMM}_changelist.{doc|src}.{lock|deprecated}?.md
    if let Some(rest) = strip_timestamp_prefix(name) {
        if let Some(after) = rest.strip_prefix("_changelist.") {
            return parse_new_format_suffix(after, name);
        }
    }

    None
}

/// Strip a `YYYYMMDDHHMM` prefix (12 digits) and return the remainder.
fn strip_timestamp_prefix(name: &str) -> Option<&str> {
    if name.len() < 12 {
        return None;
    }
    let (prefix, rest) = name.split_at(12);
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

/// Parse the suffix after `_changelist.` in new-format filenames.
///
/// Valid suffixes:
///   `doc.md`, `doc.lock.md`, `doc.deprecated.md`
///   `src.md`, `src.lock.md`, `src.deprecated.md`
fn parse_new_format_suffix(suffix: &str, full_name: &str) -> Option<ParsedChangelist> {
    let (kind, rest) = if let Some(rest) = suffix.strip_prefix("doc.") {
        (ClKind::Doc, rest)
    } else if let Some(rest) = suffix.strip_prefix("src.") {
        (ClKind::Src, rest)
    } else {
        return None;
    };

    let status = match rest {
        "md" => ClStatus::Active,
        "lock.md" => ClStatus::Locked,
        "deprecated.md" => ClStatus::Deprecated,
        _ => return None,
    };

    Some(ParsedChangelist {
        filename: full_name.to_string(),
        kind,
        status,
    })
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Scan the design_rounds directory for all changelists.
/// Only returns root-level files (not in subdirectories — those are archived).
pub fn find_changelists(design_rounds: &Path) -> Vec<ParsedChangelist> {
    let entries = match std::fs::read_dir(design_rounds) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    entries
        .flatten()
        .filter(|e| {
            e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
        })
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            parse_changelist(&name)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Convenience finders
// ---------------------------------------------------------------------------

/// Find an active (unlocked, non-deprecated) doc changelist.
pub fn find_active_doc_cl(design_rounds: &Path) -> Option<ParsedChangelist> {
    find_changelists(design_rounds)
        .into_iter()
        .find(|cl| cl.kind == ClKind::Doc && cl.status == ClStatus::Active)
}

/// Find a locked doc changelist.
pub fn find_locked_doc_cl(design_rounds: &Path) -> Option<ParsedChangelist> {
    find_changelists(design_rounds)
        .into_iter()
        .find(|cl| cl.kind == ClKind::Doc && cl.status == ClStatus::Locked)
}

/// Find an active (unlocked, non-deprecated) src changelist.
pub fn find_active_src_cl(design_rounds: &Path) -> Option<ParsedChangelist> {
    find_changelists(design_rounds)
        .into_iter()
        .find(|cl| cl.kind == ClKind::Src && cl.status == ClStatus::Active)
}

/// Find a locked src changelist.
pub fn find_locked_src_cl(design_rounds: &Path) -> Option<ParsedChangelist> {
    find_changelists(design_rounds)
        .into_iter()
        .find(|cl| cl.kind == ClKind::Src && cl.status == ClStatus::Locked)
}

/// Whether any non-deprecated changelist exists.
pub fn has_any_changelist(design_rounds: &Path) -> bool {
    find_changelists(design_rounds)
        .iter()
        .any(|cl| cl.status != ClStatus::Deprecated)
}

/// Determine the current phase based on changelist state.
pub fn current_phase(design_rounds: &Path) -> Phase {
    let cls = find_changelists(design_rounds);

    // Filter out deprecated CLs — they don't count.
    let active_doc = cls.iter().any(|cl| cl.kind == ClKind::Doc && cl.status == ClStatus::Active);
    let locked_doc = cls.iter().any(|cl| cl.kind == ClKind::Doc && cl.status == ClStatus::Locked);
    let active_src = cls.iter().any(|cl| cl.kind == ClKind::Src && cl.status == ClStatus::Active);
    let locked_src = cls.iter().any(|cl| cl.kind == ClKind::Src && cl.status == ClStatus::Locked);

    if locked_doc && locked_src {
        Phase::Done
    } else if locked_doc && active_src {
        Phase::Src
    } else if locked_doc {
        Phase::SrcPlan
    } else if active_doc {
        Phase::Doc
    } else {
        Phase::Topic
    }
}

/// Collect all frozen changelist filenames (locked + deprecated).
pub fn frozen_changelists(design_rounds: &Path) -> Vec<ParsedChangelist> {
    find_changelists(design_rounds)
        .into_iter()
        .filter(|cl| cl.status == ClStatus::Locked || cl.status == ClStatus::Deprecated)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_new_format_doc_active() {
        let cl = parse_changelist("202603071430_changelist.doc.md").unwrap();
        assert_eq!(cl.kind, ClKind::Doc);
        assert_eq!(cl.status, ClStatus::Active);
    }

    #[test]
    fn parse_new_format_doc_locked() {
        let cl = parse_changelist("202603071430_changelist.doc.lock.md").unwrap();
        assert_eq!(cl.kind, ClKind::Doc);
        assert_eq!(cl.status, ClStatus::Locked);
    }

    #[test]
    fn parse_new_format_src_deprecated() {
        let cl = parse_changelist("202603071430_changelist.src.deprecated.md").unwrap();
        assert_eq!(cl.kind, ClKind::Src);
        assert_eq!(cl.status, ClStatus::Deprecated);
    }

    #[test]
    fn parse_topic_returns_none() {
        assert!(parse_changelist("202603071430_topic.foundation.md").is_none());
    }

    #[test]
    fn parse_random_returns_none() {
        assert!(parse_changelist("README.md").is_none());
    }
}
