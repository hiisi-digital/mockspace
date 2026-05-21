//! Auto-fix application for findings carrying a [`Suggestion`] with a
//! mechanical [`Fix`] recipe.
//!
//! Per the auto-fix design memo at
//! `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`.
//!
//! ## Flow
//!
//! 1. [`plan_fixes`] collects every applicable fix from the findings, groups
//!    them per file, detects byte-range conflicts within a file (overlapping
//!    `Replace` / `Delete` / `Insert`-at-same-position), drops conflicting
//!    fixes in input order (first-finding-wins), and produces a [`FixPlan`].
//! 2. [`render_unified_diff`] renders a plan as a unified text diff for
//!    dry-run output. The format is the same one `git apply -R` consumes.
//! 3. [`apply_plan`] writes the planned changes to disk in one batch.
//!    Caller is expected to back up via the rendered diff (see the memo's
//!    "Backup" section for the contract).
//!
//! ## Conflict resolution
//!
//! Within one file, two byte-edits conflict iff their byte ranges overlap.
//! For `Insert { position }`, the byte range is `[position, position)`
//! (zero-width); two inserts at the same position conflict. The runner
//! drops the later finding in the input order, reports it in
//! [`FixPlan::conflicts`], and continues. This matches the memo's
//! "report a conflict and skip the conflicting fixes, applying the rest"
//! contract.
//!
//! ## Filesystem operations
//!
//! [`Fix::File`] variants (`Create` / `Delete` / `Rename`) are queued onto
//! [`FixPlan::file_ops`] and applied after every in-buffer edit lands. A
//! `Delete` op against a file that other fixes edited in the same plan is
//! rejected at plan time as a conflict.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mockspace_core::lint::{FileOp, Finding, Fix};

/// Caller-facing options for fix application.
#[derive(Debug, Clone, Default)]
pub struct FixOpts {
    /// When `true`, [`apply_plan`] is a no-op; the plan is meant to be
    /// rendered as a diff via [`render_unified_diff`] instead.
    pub dry_run: bool,
    /// When `Some`, only findings whose `lint_name` appears in this list
    /// are eligible. When `None`, every finding with a `suggestion.fix`
    /// is eligible.
    pub only_lints: Option<Vec<String>>,
}

/// A single per-file edit plan. The `after` field is the result of
/// applying every surviving fix to `before`; the runner writes `after`
/// when [`apply_plan`] is called with `dry_run == false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

/// Why a particular finding's fix was dropped from the plan. Reported
/// back to the caller so it can surface "this finding was not
/// auto-fixed because ...".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictReport {
    /// Name of the lint that emitted the dropped finding.
    pub lint_name: String,
    /// File the dropped fix would have touched.
    pub path: PathBuf,
    /// Plain-English reason, e.g. "overlaps with prior fix at bytes 12..18".
    pub reason: String,
}

/// Result of [`plan_fixes`]. Carries the in-buffer changes that survived
/// conflict resolution, the queued filesystem operations, the conflicts
/// the planner dropped, and a tally for the caller's reporting.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FixPlan {
    pub file_changes: Vec<FileChange>,
    pub file_ops: Vec<FileOp>,
    pub conflicts: Vec<ConflictReport>,
    /// Number of findings inspected but not eligible (no `suggestion.fix`
    /// at all, or filtered out by `only_lints`). Distinct from
    /// `conflicts.len()` which counts findings whose fix WAS eligible but
    /// then dropped during conflict resolution.
    pub skipped_advisory: usize,
    /// Total leaf byte-edits + file ops that survived to `apply_plan`.
    pub fixes_applied: usize,
}

/// Errors arising during fix planning or application.
#[derive(Debug)]
pub enum FixError {
    /// A `Replace` / `Delete` named byte offsets outside the source.
    OutOfRangeEdit {
        path: PathBuf,
        start: usize,
        end: usize,
        len: usize,
    },
    /// A byte offset fell on a non-UTF-8-character boundary.
    NotCharBoundary { path: PathBuf, offset: usize },
    /// IO error reading or writing a source file.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// File-level op conflicts (delete-then-edit, create-existing, etc).
    FileOpConflict { reason: String },
    /// A `Fix::Multi` contains internally-overlapping leaf edits on the same
    /// file. Reported as an error (not a soft conflict) because Multi must
    /// apply atomically per the design memo; a Multi whose own leaves
    /// conflict cannot satisfy that contract.
    MultiAtomicityViolation {
        lint_name: String,
        path: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for FixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixError::OutOfRangeEdit {
                path,
                start,
                end,
                len,
            } => write!(
                f,
                "fix edit {start}..{end} out of range in {} ({len} bytes)",
                path.display()
            ),
            FixError::NotCharBoundary { path, offset } => write!(
                f,
                "fix offset {offset} not on UTF-8 boundary in {}",
                path.display()
            ),
            FixError::Io { path, source } => {
                write!(f, "io error on {}: {source}", path.display())
            }
            FixError::FileOpConflict { reason } => write!(f, "file-op conflict: {reason}"),
            FixError::MultiAtomicityViolation {
                lint_name,
                path,
                reason,
            } => write!(
                f,
                "Fix::Multi atomicity violation from `{lint_name}` on {}: {reason}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FixError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FixError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// A single leaf byte-edit collected from a finding's fix tree.
#[derive(Debug, Clone)]
struct LeafEdit {
    lint_name: String,
    start: usize,
    end: usize,
    replacement: String,
}

/// Walk a `Fix` tree and collect every leaf byte-edit, recording file
/// ops separately. The `path` parameter is the path the edits should
/// land on, taken from the finding's span. `Fix::File` ops carry their
/// own path inside `FileOp` and are pushed onto `file_ops` as-is.
fn collect_leaves(
    fix: &Fix,
    lint_name: &str,
    out_edits: &mut Vec<LeafEdit>,
    out_ops: &mut Vec<FileOp>,
) {
    match fix {
        Fix::Replace {
            start,
            end,
            replacement,
        } => {
            out_edits.push(LeafEdit {
                lint_name: lint_name.to_owned(),
                start: *start,
                end: *end,
                replacement: replacement.to_string(),
            });
        }
        Fix::Insert { position, text } => {
            // An Insert is a zero-width edit at `position`.
            out_edits.push(LeafEdit {
                lint_name: lint_name.to_owned(),
                start: *position,
                end: *position,
                replacement: text.to_string(),
            });
        }
        Fix::Delete { start, end } => {
            out_edits.push(LeafEdit {
                lint_name: lint_name.to_owned(),
                start: *start,
                end: *end,
                replacement: String::new(),
            });
        }
        Fix::Multi { fixes } => {
            for inner in fixes {
                collect_leaves(inner, lint_name, out_edits, out_ops);
            }
        }
        Fix::File { op } => {
            out_ops.push(op.clone());
        }
    }
}

/// Plan the fix application across `findings`. Reads each finding's
/// source file once, applies surviving edits in reverse byte-offset
/// order (so earlier offsets do not shift), and returns the resulting
/// plan.
///
/// `project_root` resolves the relative paths inside each finding's
/// [`Span`]. Pass the same root the engine ran against.
/// A finding's complete set of leaf edits on one file, treated as one
/// atomic group for conflict resolution. A finding whose fix is a `Multi`
/// produces one group containing every leaf; a finding whose fix is a
/// single `Replace` / `Insert` / `Delete` produces a group with one leaf.
/// Either every leaf in the group lands or none do (the memo's
/// "applied atomically" contract).
#[derive(Debug, Clone)]
struct FindingEditGroup {
    lint_name: String,
    leaves: Vec<LeafEdit>,
}

pub fn plan_fixes(
    project_root: &Path,
    findings: &[Finding],
    opts: &FixOpts,
) -> Result<FixPlan, FixError> {
    let mut plan = FixPlan::default();

    // Collect eligible findings into per-file edit groups. Groups
    // preserve the per-finding boundary so atomicity of Fix::Multi can
    // be enforced at conflict-resolution time.
    let mut by_file: BTreeMap<PathBuf, Vec<FindingEditGroup>> = BTreeMap::new();
    let mut file_ops: Vec<FileOp> = Vec::new();

    for finding in findings.iter() {
        let fix = match finding.suggestion.as_ref().and_then(|s| s.fix.as_ref()) {
            Some(fix) => fix,
            None => {
                plan.skipped_advisory += 1;
                continue;
            }
        };
        if let Some(only) = opts.only_lints.as_ref() {
            if !only.iter().any(|n| n == finding.lint_name.as_ref()) {
                plan.skipped_advisory += 1;
                continue;
            }
        }
        let path = normalize_relative_path(project_root, &finding.span.file);
        let mut edits: Vec<LeafEdit> = Vec::new();
        collect_leaves(fix, &finding.lint_name, &mut edits, &mut file_ops);
        if !edits.is_empty() {
            // Intra-group conflict check: if a Fix::Multi has leaves
            // that overlap each other on this file, the contract is
            // unsatisfiable (atomic application would require both
            // edits in conflicting positions). Surface as a hard error
            // so the lint author can fix the Multi at its source.
            for i in 0..edits.len() {
                for j in (i + 1)..edits.len() {
                    if ranges_conflict(&edits[i], &edits[j]) {
                        return Err(FixError::MultiAtomicityViolation {
                            lint_name: finding.lint_name.to_string(),
                            path: path.clone(),
                            reason: format!(
                                "leaves at bytes {}..{} and {}..{} overlap within one Fix::Multi",
                                edits[i].start, edits[i].end, edits[j].start, edits[j].end
                            ),
                        });
                    }
                }
            }
            by_file
                .entry(path)
                .or_default()
                .push(FindingEditGroup {
                    lint_name: finding.lint_name.to_string(),
                    leaves: edits,
                });
        }
    }

    // Per file: detect inter-group overlaps in input order, drop later
    // conflicting groups as one unit, apply surviving groups' leaves
    // in reverse byte-offset order.
    for (path, groups) in by_file {
        let source = fs::read_to_string(&path).map_err(|source| FixError::Io {
            path: path.clone(),
            source,
        })?;

        // Validate each group's leaf bounds first; out-of-range is a
        // planner-detected lint bug, not a soft conflict.
        for group in &groups {
            for edit in &group.leaves {
                if edit.start > source.len() || edit.end > source.len() || edit.start > edit.end {
                    return Err(FixError::OutOfRangeEdit {
                        path: path.clone(),
                        start: edit.start,
                        end: edit.end,
                        len: source.len(),
                    });
                }
                if !source.is_char_boundary(edit.start) {
                    return Err(FixError::NotCharBoundary {
                        path: path.clone(),
                        offset: edit.start,
                    });
                }
                if !source.is_char_boundary(edit.end) {
                    return Err(FixError::NotCharBoundary {
                        path: path.clone(),
                        offset: edit.end,
                    });
                }
            }
        }

        let mut survivors: Vec<LeafEdit> = Vec::new();
        let mut applied_groups: usize = 0;
        for group in groups {
            // A group conflicts iff any of its leaves overlaps any
            // already-surviving leaf. On conflict the whole group is
            // dropped (atomicity), and one ConflictReport is recorded
            // pointing at the first overlapping pair.
            let conflict = group.leaves.iter().find_map(|edit| {
                survivors
                    .iter()
                    .find(|s| ranges_conflict(s, edit))
                    .map(|prior| (edit.clone(), prior.clone()))
            });
            if let Some((own, prior)) = conflict {
                plan.conflicts.push(ConflictReport {
                    lint_name: group.lint_name.clone(),
                    path: path.clone(),
                    reason: format!(
                        "leaf at bytes {}..{} overlaps with prior fix from `{}` at bytes {}..{}",
                        own.start, own.end, prior.lint_name, prior.start, prior.end
                    ),
                });
                continue;
            }
            survivors.extend(group.leaves);
            applied_groups += 1;
        }

        if survivors.is_empty() {
            // Groups may have all been dropped; even if `groups` was
            // non-empty originally, no edits survive.
            let _ = applied_groups;
            continue;
        }

        // Apply in reverse offset order so earlier-byte edits are not
        // shifted by later ones.
        survivors.sort_by(|a, b| b.start.cmp(&a.start));

        let mut buf = source.clone();
        for edit in &survivors {
            buf.replace_range(edit.start..edit.end, &edit.replacement);
        }

        plan.fixes_applied += survivors.len();
        plan.file_changes.push(FileChange {
            path,
            before: source,
            after: buf,
        });
    }

    // Validate file-ops against the in-buffer changes. Specifically:
    // a `FileOp::Delete { path: P }` while a byte-edit also targeted P
    // is a conflict ("delete then edit" makes no sense).
    for op in &file_ops {
        if let FileOp::Delete { path } = op {
            let resolved = normalize_relative_path(project_root, Path::new(path.as_ref()));
            if plan.file_changes.iter().any(|c| c.path == resolved) {
                return Err(FixError::FileOpConflict {
                    reason: format!(
                        "Fix::File::Delete of `{}` collides with an in-buffer edit on the same path",
                        path
                    ),
                });
            }
        }
    }

    plan.fixes_applied += file_ops.len();
    plan.file_ops = file_ops;
    Ok(plan)
}

/// Join a finding-or-fileop path onto `project_root` and strip
/// `Component::CurDir` segments (`.`) so `./foo.rs` and `foo.rs`
/// resolve to the same `PathBuf` for collision comparisons.
///
/// Does NOT canonicalize (which would require the file to exist on disk
/// and would resolve symlinks in ways that surprise the caller).
/// Absolute paths inside the input replace `project_root` per
/// `Path::join` semantics; this is intentional so lints emitting
/// absolute paths still resolve consistently.
fn normalize_relative_path(project_root: &Path, rel: impl AsRef<Path>) -> PathBuf {
    use std::path::Component;
    let joined = project_root.join(rel);
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Two leaf edits conflict iff their `[start, end)` ranges overlap.
/// For `Insert` (zero-width at `position`), two inserts at the same
/// position are a conflict; an insert that lies strictly inside a
/// replace/delete range is also a conflict.
fn ranges_conflict(a: &LeafEdit, b: &LeafEdit) -> bool {
    // Treat zero-width inserts as occupying [pos, pos+1) for overlap
    // purposes against ranges, but two inserts at the same position
    // must explicitly tie.
    let (a_start, a_end) = (a.start, a.end);
    let (b_start, b_end) = (b.start, b.end);
    if a_start == a_end && b_start == b_end {
        return a_start == b_start;
    }
    // Otherwise standard half-open overlap: `a_start < b_end && b_start < a_end`.
    // A zero-width insert overlaps with a non-empty range iff
    // a_start lies strictly inside (b_start, b_end). Touching either
    // boundary is fine.
    if a_start == a_end {
        return a_start > b_start && a_start < b_end;
    }
    if b_start == b_end {
        return b_start > a_start && b_start < a_end;
    }
    a_start < b_end && b_start < a_end
}

/// Apply the planned writes to disk. No-op when `opts.dry_run == true`.
/// File-level ops apply after every byte-edit lands.
pub fn apply_plan(plan: &FixPlan, opts: &FixOpts, project_root: &Path) -> Result<(), FixError> {
    if opts.dry_run {
        return Ok(());
    }
    for change in &plan.file_changes {
        fs::write(&change.path, &change.after).map_err(|source| FixError::Io {
            path: change.path.clone(),
            source,
        })?;
    }
    for op in &plan.file_ops {
        match op {
            FileOp::Create { path, content } => {
                let resolved = project_root.join(path.as_ref());
                if resolved.exists() {
                    return Err(FixError::FileOpConflict {
                        reason: format!("Fix::File::Create on existing path {}", path),
                    });
                }
                if let Some(parent) = resolved.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).map_err(|source| FixError::Io {
                            path: parent.to_owned(),
                            source,
                        })?;
                    }
                }
                fs::write(&resolved, content.as_ref()).map_err(|source| FixError::Io {
                    path: resolved,
                    source,
                })?;
            }
            FileOp::Delete { path } => {
                let resolved = project_root.join(path.as_ref());
                fs::remove_file(&resolved).map_err(|source| FixError::Io {
                    path: resolved,
                    source,
                })?;
            }
            FileOp::Rename { from, to } => {
                let from_p = project_root.join(from.as_ref());
                let to_p = project_root.join(to.as_ref());
                if to_p.exists() {
                    return Err(FixError::FileOpConflict {
                        reason: format!("Fix::File::Rename target exists: {}", to),
                    });
                }
                fs::rename(&from_p, &to_p).map_err(|source| FixError::Io {
                    path: from_p,
                    source,
                })?;
            }
        }
    }
    Ok(())
}

/// Render the plan as a unified diff suitable for `git apply -R`.
///
/// Minimal renderer: one hunk per changed file, full file content
/// before and after, no context-line elision. Files lacking a trailing
/// newline get the `\ No newline at end of file` sentinel after the
/// last `-`/`+` line per the diff format, so `git apply -R` round-trips
/// byte-for-byte regardless of trailing-newline state.
pub fn render_unified_diff(plan: &FixPlan) -> String {
    let mut out = String::new();
    for change in &plan.file_changes {
        let path_str = change.path.display().to_string();
        out.push_str(&format!("--- a/{path_str}\n"));
        out.push_str(&format!("+++ b/{path_str}\n"));

        let before_lines: Vec<&str> = if change.before.is_empty() {
            Vec::new()
        } else {
            change.before.split_inclusive('\n').collect()
        };
        let after_lines: Vec<&str> = if change.after.is_empty() {
            Vec::new()
        } else {
            change.after.split_inclusive('\n').collect()
        };
        out.push_str(&format!(
            "@@ -1,{} +1,{} @@\n",
            before_lines.len(),
            after_lines.len()
        ));
        for (i, line) in before_lines.iter().enumerate() {
            let is_last = i + 1 == before_lines.len();
            out.push('-');
            out.push_str(line);
            if !line.ends_with('\n') {
                out.push('\n');
                if is_last {
                    out.push_str("\\ No newline at end of file\n");
                }
            }
        }
        for (i, line) in after_lines.iter().enumerate() {
            let is_last = i + 1 == after_lines.len();
            out.push('+');
            out.push_str(line);
            if !line.ends_with('\n') {
                out.push('\n');
                if is_last {
                    out.push_str("\\ No newline at end of file\n");
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::{Severity, Span, Suggestion};
    use std::borrow::Cow;

    fn finding_with_fix(
        path: &str,
        lint_name: &'static str,
        fix: Fix,
    ) -> Finding {
        Finding {
            lint_name: Cow::Borrowed(lint_name),
            rule_id: None,
            plugin_id: None,
            severity: Severity::Warn,
            impact: None,
            category: None,
            message: Cow::Borrowed("test"),
            span: Span::single_line(path, 1, 0, 1),
            hint: None,
            help: None,
            suggestion: Some(Suggestion {
                description: Cow::Borrowed("apply"),
                fix: Some(fix),
            }),
            related_spans: Vec::new(),
            metadata: None,
        }
    }

    fn finding_advisory(lint_name: &'static str) -> Finding {
        Finding {
            lint_name: Cow::Borrowed(lint_name),
            rule_id: None,
            plugin_id: None,
            severity: Severity::Warn,
            impact: None,
            category: None,
            message: Cow::Borrowed("test"),
            span: Span::single_line("a.rs", 1, 0, 1),
            hint: Some(Cow::Borrowed("consider X")),
            help: None,
            suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        }
    }

    fn write_source(root: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn replace_edits_apply_in_reverse_order() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "let x = 1; let y = 2;");
        // Two non-overlapping replaces. Reverse-order application means
        // the second-byte-offset edit must succeed regardless of where
        // the first lands.
        let f1 = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 8,
                end: 9,
                replacement: Cow::Borrowed("9"),
            },
        );
        let f2 = finding_with_fix(
            "a.rs",
            "lint-b",
            Fix::Replace {
                start: 19,
                end: 20,
                replacement: Cow::Borrowed("8"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f1, f2], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes.len(), 1);
        assert_eq!(plan.file_changes[0].after, "let x = 9; let y = 8;");
        assert_eq!(plan.fixes_applied, 2);
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn overlapping_edits_drop_later_finding() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "hello world");
        let f1 = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 0,
                end: 5,
                replacement: Cow::Borrowed("HELLO"),
            },
        );
        let f2 = finding_with_fix(
            "a.rs",
            "lint-b",
            Fix::Replace {
                start: 2,
                end: 7,
                replacement: Cow::Borrowed("XXXXX"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f1, f2], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes.len(), 1);
        assert_eq!(plan.file_changes[0].after, "HELLO world");
        assert_eq!(plan.fixes_applied, 1);
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].lint_name, "lint-b");
        assert!(plan.conflicts[0].reason.contains("lint-a"));
    }

    #[test]
    fn insert_at_same_position_is_a_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "ab");
        let f1 = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Insert {
                position: 1,
                text: Cow::Borrowed("X"),
            },
        );
        let f2 = finding_with_fix(
            "a.rs",
            "lint-b",
            Fix::Insert {
                position: 1,
                text: Cow::Borrowed("Y"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f1, f2], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes[0].after, "aXb");
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].lint_name, "lint-b");
    }

    #[test]
    fn insert_at_distinct_positions_both_land() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "abc");
        let f1 = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Insert {
                position: 1,
                text: Cow::Borrowed("X"),
            },
        );
        let f2 = finding_with_fix(
            "a.rs",
            "lint-b",
            Fix::Insert {
                position: 2,
                text: Cow::Borrowed("Y"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f1, f2], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes[0].after, "aXbYc");
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn delete_compacts_source() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "alpha beta gamma");
        let f1 = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Delete { start: 5, end: 10 },
        );
        let plan = plan_fixes(tmp.path(), &[f1], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes[0].after, "alpha gamma");
    }

    #[test]
    fn multi_fix_collects_all_leaves() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "12345");
        let f = finding_with_fix(
            "a.rs",
            "multi-lint",
            Fix::Multi {
                fixes: vec![
                    Fix::Replace {
                        start: 0,
                        end: 1,
                        replacement: Cow::Borrowed("A"),
                    },
                    Fix::Replace {
                        start: 4,
                        end: 5,
                        replacement: Cow::Borrowed("E"),
                    },
                ],
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes[0].after, "A234E");
        assert_eq!(plan.fixes_applied, 2);
    }

    #[test]
    fn advisory_finding_counts_as_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = plan_fixes(
            tmp.path(),
            &[finding_advisory("lint-a")],
            &FixOpts::default(),
        )
        .unwrap();
        assert_eq!(plan.skipped_advisory, 1);
        assert!(plan.file_changes.is_empty());
        assert_eq!(plan.fixes_applied, 0);
    }

    #[test]
    fn only_lints_filter_excludes_other_findings() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "abc");
        let f_kept = finding_with_fix(
            "a.rs",
            "kept",
            Fix::Replace {
                start: 0,
                end: 1,
                replacement: Cow::Borrowed("A"),
            },
        );
        let f_dropped = finding_with_fix(
            "a.rs",
            "dropped",
            Fix::Replace {
                start: 1,
                end: 2,
                replacement: Cow::Borrowed("B"),
            },
        );
        let opts = FixOpts {
            dry_run: false,
            only_lints: Some(vec!["kept".to_string()]),
        };
        let plan = plan_fixes(tmp.path(), &[f_kept, f_dropped], &opts).unwrap();
        assert_eq!(plan.file_changes[0].after, "Abc");
        assert_eq!(plan.fixes_applied, 1);
        assert_eq!(plan.skipped_advisory, 1);
    }

    #[test]
    fn out_of_range_edit_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "abc");
        let f = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 0,
                end: 100,
                replacement: Cow::Borrowed("X"),
            },
        );
        let err = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap_err();
        assert!(matches!(err, FixError::OutOfRangeEdit { .. }));
    }

    #[test]
    fn non_char_boundary_offset_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        // Multi-byte char: first byte at 0, next char boundary at 4.
        write_source(tmp.path(), "a.rs", "🦀x");
        let f = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 1,
                end: 2,
                replacement: Cow::Borrowed("Y"),
            },
        );
        let err = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap_err();
        assert!(matches!(err, FixError::NotCharBoundary { .. }));
    }

    #[test]
    fn apply_plan_writes_file_when_not_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_source(tmp.path(), "a.rs", "hello");
        let f = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 0,
                end: 5,
                replacement: Cow::Borrowed("HELLO"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        apply_plan(&plan, &FixOpts::default(), tmp.path()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "HELLO");
    }

    #[test]
    fn dry_run_does_not_write() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_source(tmp.path(), "a.rs", "hello");
        let f = finding_with_fix(
            "a.rs",
            "lint-a",
            Fix::Replace {
                start: 0,
                end: 5,
                replacement: Cow::Borrowed("HELLO"),
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        let opts = FixOpts {
            dry_run: true,
            only_lints: None,
        };
        apply_plan(&plan, &opts, tmp.path()).unwrap();
        // File still has the original contents.
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        // But the plan's `after` field reflects what would have been written.
        assert_eq!(plan.file_changes[0].after, "HELLO");
    }

    #[test]
    fn unified_diff_renders_changed_file() {
        let plan = FixPlan {
            file_changes: vec![FileChange {
                path: PathBuf::from("a.rs"),
                before: "hello\n".to_string(),
                after: "HELLO\n".to_string(),
            }],
            file_ops: Vec::new(),
            conflicts: Vec::new(),
            skipped_advisory: 0,
            fixes_applied: 1,
        };
        let diff = render_unified_diff(&plan);
        assert!(diff.contains("--- a/a.rs"));
        assert!(diff.contains("+++ b/a.rs"));
        assert!(diff.contains("-hello"));
        assert!(diff.contains("+HELLO"));
    }

    #[test]
    fn file_op_create_writes_new_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "fn x() {}\n");
        let f = finding_with_fix(
            "a.rs",
            "scaffold-lint",
            Fix::File {
                op: FileOp::Create {
                    path: Cow::Borrowed("BACKLOG.md"),
                    content: Cow::Borrowed("# Backlog\n"),
                },
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_ops.len(), 1);
        apply_plan(&plan, &FixOpts::default(), tmp.path()).unwrap();
        let created = tmp.path().join("BACKLOG.md");
        assert!(created.exists());
        assert_eq!(fs::read_to_string(&created).unwrap(), "# Backlog\n");
    }

    #[test]
    fn file_op_create_existing_path_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "x");
        write_source(tmp.path(), "already.md", "exists");
        let f = finding_with_fix(
            "a.rs",
            "scaffold-lint",
            Fix::File {
                op: FileOp::Create {
                    path: Cow::Borrowed("already.md"),
                    content: Cow::Borrowed("nope"),
                },
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        let err = apply_plan(&plan, &FixOpts::default(), tmp.path()).unwrap_err();
        assert!(matches!(err, FixError::FileOpConflict { .. }));
    }

    #[test]
    fn file_op_delete_collides_with_buffer_edit_on_same_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "doomed.rs", "let x = 1;");
        let f_edit = finding_with_fix(
            "doomed.rs",
            "edit-lint",
            Fix::Replace {
                start: 4,
                end: 5,
                replacement: Cow::Borrowed("y"),
            },
        );
        let f_delete = finding_with_fix(
            "doomed.rs",
            "delete-lint",
            Fix::File {
                op: FileOp::Delete {
                    path: Cow::Borrowed("doomed.rs"),
                },
            },
        );
        let err = plan_fixes(tmp.path(), &[f_edit, f_delete], &FixOpts::default()).unwrap_err();
        assert!(matches!(err, FixError::FileOpConflict { .. }));
    }

    #[test]
    fn intra_multi_overlap_returns_atomicity_violation() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "abcdef");
        let f = finding_with_fix(
            "a.rs",
            "buggy-multi",
            Fix::Multi {
                fixes: vec![
                    Fix::Replace {
                        start: 0,
                        end: 3,
                        replacement: Cow::Borrowed("X"),
                    },
                    Fix::Replace {
                        start: 2,
                        end: 5,
                        replacement: Cow::Borrowed("Y"),
                    },
                ],
            },
        );
        let err = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap_err();
        match err {
            FixError::MultiAtomicityViolation {
                lint_name, reason, ..
            } => {
                assert_eq!(lint_name, "buggy-multi");
                assert!(reason.contains("overlap"));
            }
            other => panic!("expected MultiAtomicityViolation, got {other:?}"),
        }
    }

    #[test]
    fn multi_with_inter_finding_conflict_drops_whole_group() {
        // Finding A has a Multi { Replace 0..1, Replace 5..6 } and lands
        // first. Finding B has a Replace 0..6 that overlaps the first
        // leaf of A's Multi. With atomicity, A's whole Multi is the
        // surviving group; B is dropped wholesale. The reverse order
        // (B first, then A) is the more interesting test: a single
        // surviving non-Multi blocks a later Multi entirely (not
        // partially).
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "abcdef");
        let f_first = finding_with_fix(
            "a.rs",
            "blocker",
            Fix::Replace {
                start: 0,
                end: 6,
                replacement: Cow::Borrowed("ZZZZZZ"),
            },
        );
        let f_multi = finding_with_fix(
            "a.rs",
            "multi-loser",
            Fix::Multi {
                fixes: vec![
                    Fix::Replace {
                        start: 0,
                        end: 1,
                        replacement: Cow::Borrowed("X"),
                    },
                    Fix::Replace {
                        start: 5,
                        end: 6,
                        replacement: Cow::Borrowed("Y"),
                    },
                ],
            },
        );
        let plan = plan_fixes(tmp.path(), &[f_first, f_multi], &FixOpts::default()).unwrap();
        assert_eq!(plan.file_changes[0].after, "ZZZZZZ");
        // The Multi was dropped as one unit; one conflict report names it.
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].lint_name, "multi-loser");
        // Neither half of the Multi landed.
        assert!(!plan.file_changes[0].after.contains('X'));
        assert!(!plan.file_changes[0].after.contains('Y'));
    }

    #[test]
    fn current_dir_prefix_normalises_for_collision_detection() {
        // A finding spans `a.rs` while a sibling FileOp::Delete targets
        // `./a.rs`. After normalisation both resolve to the same path
        // and the delete-then-edit collision is detected.
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "a.rs", "hi");
        let f_edit = finding_with_fix(
            "a.rs",
            "edit-lint",
            Fix::Replace {
                start: 0,
                end: 2,
                replacement: Cow::Borrowed("HI"),
            },
        );
        let f_delete = finding_with_fix(
            "b.rs",
            "delete-lint",
            Fix::File {
                op: FileOp::Delete {
                    path: Cow::Borrowed("./a.rs"),
                },
            },
        );
        let err = plan_fixes(tmp.path(), &[f_edit, f_delete], &FixOpts::default()).unwrap_err();
        assert!(matches!(err, FixError::FileOpConflict { .. }));
    }

    #[test]
    fn diff_marks_missing_trailing_newline() {
        // Source without trailing newline. Plan reports the missing
        // newline both before and after via the standard sentinel so
        // `git apply -R` round-trips byte-for-byte.
        let plan = FixPlan {
            file_changes: vec![FileChange {
                path: PathBuf::from("a.rs"),
                before: "hello".to_string(), // no trailing newline
                after: "HELLO".to_string(),  // no trailing newline
            }],
            file_ops: Vec::new(),
            conflicts: Vec::new(),
            skipped_advisory: 0,
            fixes_applied: 1,
        };
        let diff = render_unified_diff(&plan);
        // Both sides must carry the sentinel.
        assert_eq!(
            diff.matches("\\ No newline at end of file").count(),
            2,
            "expected sentinel on both - and + sides, got: {diff:?}"
        );
    }

    #[test]
    fn diff_omits_sentinel_when_both_sides_end_in_newline() {
        let plan = FixPlan {
            file_changes: vec![FileChange {
                path: PathBuf::from("a.rs"),
                before: "hello\n".to_string(),
                after: "HELLO\n".to_string(),
            }],
            file_ops: Vec::new(),
            conflicts: Vec::new(),
            skipped_advisory: 0,
            fixes_applied: 1,
        };
        let diff = render_unified_diff(&plan);
        assert!(
            !diff.contains("\\ No newline at end of file"),
            "did not expect sentinel, got: {diff:?}"
        );
    }

    #[test]
    fn file_op_rename_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        write_source(tmp.path(), "old.rs", "fn x() {}\n");
        let f = finding_with_fix(
            "old.rs",
            "rename-lint",
            Fix::File {
                op: FileOp::Rename {
                    from: Cow::Borrowed("old.rs"),
                    to: Cow::Borrowed("new.rs"),
                },
            },
        );
        let plan = plan_fixes(tmp.path(), &[f], &FixOpts::default()).unwrap();
        apply_plan(&plan, &FixOpts::default(), tmp.path()).unwrap();
        assert!(!tmp.path().join("old.rs").exists());
        assert!(tmp.path().join("new.rs").exists());
    }
}
