//! Design rounds discovery + parsing.
//!
//! Walks `mock/design_rounds/` and assembles a [`DesignRoundsView`]: one
//! [`DesignRound`] per immediate subdirectory whose name resembles a round
//! timestamp (12-digit YYYYMMDDHHMM). Per-round state is inferred from the
//! files present rather than from a separate manifest, because the legacy
//! mockspace convention does not store explicit state outside the file
//! naming pattern.
//!
//! Inference rules:
//!
//! - Any `*.lock.md` file in the round directory → `RoundState::Locked`
//!   and `locked = true`.
//! - A `closed/` subdirectory or a name like `closed-<timestamp>/` →
//!   `RoundState::Closed`.
//! - A file matching `doc-*.lock.md` or `*-doc.lock.md` → `doc_cl`.
//! - A file matching `src-*.lock.md` or `*-src.lock.md` → `src_cl`.
//! - Falls back to `RoundState::Topic` when nothing is locked but a
//!   `topic.md` is present; otherwise the round is reported with the
//!   default state and any locked-flag information that was inferable.
//!
//! Errors reading individual files are logged to stderr and the round
//! still lands with what was inferred.

use std::path::{Path, PathBuf};

use crate::project::{DesignRound, DesignRoundsView, RoundState};

/// Walk `<root>/mock/design_rounds/` and build a [`DesignRoundsView`].
/// Returns an empty view (with the path captured) if the directory does
/// not exist.
pub fn discover_design_rounds(root: &Path) -> DesignRoundsView {
    let rounds_root = root.join("mock").join("design_rounds");
    let mut rounds: Vec<DesignRound> = Vec::new();

    let read = match std::fs::read_dir(&rounds_root) {
        Ok(r) => r,
        Err(_) => {
            return DesignRoundsView {
                root: rounds_root,
                rounds,
            };
        },
    };

    for entry in read.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let (timestamp, closed_from_name) = if let Some(rest) = name.strip_prefix("closed-") {
            (rest.to_string(), true)
        } else {
            (name.to_string(), false)
        };
        // Permit non-conformant directory names; the workflow-state lint
        // is the one that complains about non-matching timestamps.
        let round = inspect_round(&path, timestamp, closed_from_name);
        rounds.push(round);
    }
    rounds.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    DesignRoundsView {
        root: rounds_root,
        rounds,
    }
}

fn inspect_round(dir: &Path, timestamp: String, closed_from_name: bool) -> DesignRound {
    let mut doc_cl: Option<PathBuf> = None;
    let mut src_cl: Option<PathBuf> = None;
    let mut any_lock = false;
    let mut topic_present = false;
    let mut closed_subdir = false;

    let Ok(read) = std::fs::read_dir(dir) else {
        return DesignRound {
            timestamp,
            state: if closed_from_name { RoundState::Closed } else { RoundState::Topic },
            doc_cl: None,
            src_cl: None,
            locked: false,
        };
    };

    // Collect lock files first so we can sort deterministically; read_dir
    // order is platform-dependent.
    let mut lock_files: Vec<PathBuf> = Vec::new();
    for entry in read.flatten() {
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_type.is_dir() {
            if name == "closed" {
                closed_subdir = true;
            }
            continue;
        }
        if name == "topic.md" {
            topic_present = true;
            continue;
        }
        if name.ends_with(".lock.md") {
            any_lock = true;
            lock_files.push(path);
        }
    }
    lock_files.sort();

    let mut unclassified: Vec<PathBuf> = Vec::new();
    for path in &lock_files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let stem = name.trim_end_matches(".lock.md");
        // Tighten classifiers: require an explicit `-` separator so
        // "docs", "doctor", "srcfoo" don't get misclassified as doc/src CLs.
        let is_doc = stem == "doc"
            || stem.starts_with("doc-")
            || stem.ends_with("-doc")
            || stem.contains("-doc-");
        let is_src = stem == "src"
            || stem.starts_with("src-")
            || stem.ends_with("-src")
            || stem.contains("-src-");
        if is_doc && doc_cl.is_none() {
            doc_cl = Some(path.clone());
        } else if is_src && src_cl.is_none() {
            src_cl = Some(path.clone());
        } else if !is_doc && !is_src {
            unclassified.push(path.clone());
        }
    }
    // First unclassified lock backs into the doc_cl slot if unset.
    // Surface a warning if more than one unclassified lock was present so
    // the consumer can tighten round naming.
    if let Some(first) = unclassified.first() {
        if doc_cl.is_none() {
            doc_cl = Some(first.clone());
        }
        if unclassified.len() > 1 {
            eprintln!(
                "warning: round at {} has {} unclassified .lock.md files; only the first ({}) was kept as doc_cl",
                dir.display(),
                unclassified.len(),
                first.display()
            );
        }
    }

    let state = if closed_from_name || closed_subdir {
        RoundState::Closed
    } else if any_lock {
        RoundState::Locked
    } else if src_cl.is_some() {
        RoundState::Src
    } else if doc_cl.is_some() {
        RoundState::Doc
    } else if topic_present {
        RoundState::Topic
    } else {
        RoundState::Topic
    };

    DesignRound {
        timestamp,
        state,
        doc_cl,
        src_cl,
        locked: any_lock,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;

    use super::*;

    fn write(p: &Path, contents: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn missing_directory_yields_empty_view() {
        let tmp = tempfile::tempdir().unwrap();
        let view = discover_design_rounds(tmp.path());
        assert!(view.rounds.is_empty());
    }

    #[test]
    fn round_with_doc_lock_is_locked() {
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/202605211200");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("topic.md"), "# topic");
        write(&round_dir.join("foo-doc.lock.md"), "# doc");
        let view = discover_design_rounds(tmp.path());
        assert_eq!(view.rounds.len(), 1);
        let r = &view.rounds[0];
        assert_eq!(r.timestamp, "202605211200");
        assert_eq!(r.state, RoundState::Locked);
        assert!(r.locked);
        assert!(r.doc_cl.is_some());
        assert!(r.src_cl.is_none());
    }

    #[test]
    fn round_with_only_topic_is_topic_state() {
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/202605211201");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("topic.md"), "# topic");
        let view = discover_design_rounds(tmp.path());
        assert_eq!(view.rounds[0].state, RoundState::Topic);
        assert!(!view.rounds[0].locked);
    }

    #[test]
    fn closed_prefix_directory_is_closed_state() {
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/closed-202605211100");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("foo-doc.lock.md"), "# doc");
        let view = discover_design_rounds(tmp.path());
        assert_eq!(view.rounds[0].state, RoundState::Closed);
        assert_eq!(view.rounds[0].timestamp, "202605211100");
    }

    #[test]
    fn rounds_sorted_by_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let r1 = tmp.path().join("mock/design_rounds/202605211400");
        let r2 = tmp.path().join("mock/design_rounds/202605211200");
        let r3 = tmp.path().join("mock/design_rounds/202605211300");
        fs::create_dir_all(&r1).unwrap();
        fs::create_dir_all(&r2).unwrap();
        fs::create_dir_all(&r3).unwrap();
        let view = discover_design_rounds(tmp.path());
        let timestamps: Vec<_> = view.rounds.iter().map(|r| r.timestamp.clone()).collect();
        assert_eq!(timestamps, vec![
            "202605211200",
            "202605211300",
            "202605211400"
        ]);
    }

    #[test]
    fn classifier_requires_hyphen_separator() {
        // `docs.lock.md`, `srcfoo.lock.md` should NOT be classified as
        // doc/src CL purely on starts_with; the prior heuristic would
        // mis-classify these.
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/202605211600");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("docs.lock.md"), "");
        write(&round_dir.join("srcfoo.lock.md"), "");
        let view = discover_design_rounds(tmp.path());
        let r = &view.rounds[0];
        // Neither file qualifies as a typed CL, so the first sorted
        // unclassified lock backs into doc_cl as a fallback. Both
        // files appear; the fallback picks the alphabetically first.
        assert!(r.doc_cl.is_some());
        assert!(r.src_cl.is_none());
        assert!(r.locked);
    }

    #[test]
    fn separates_doc_and_src_locks() {
        let tmp = tempfile::tempdir().unwrap();
        let round_dir = tmp.path().join("mock/design_rounds/202605211500");
        fs::create_dir_all(&round_dir).unwrap();
        write(&round_dir.join("foo-doc.lock.md"), "");
        write(&round_dir.join("foo-src.lock.md"), "");
        let view = discover_design_rounds(tmp.path());
        let r = &view.rounds[0];
        assert!(r.doc_cl.is_some(), "doc_cl should be set");
        assert!(r.src_cl.is_some(), "src_cl should be set");
        assert!(r.locked);
    }
}
