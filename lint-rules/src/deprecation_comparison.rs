//! Cross-crate lint: active changelists must compare to deprecated ones.
//!
//! When a changelist is deprecated and a new one created in its place,
//! the new changelist must include a "Comparison to deprecated changelist"
//! section that:
//! 1. Contains a `## Comparison to deprecated changelist` header
//! 2. Has `###` subheaders matching each `##` header in the deprecated CL
//! 3. Mentions all backtick-wrapped type/trait/macro names from the deprecated CL
//!
//! Severity: PUSH_GATE (warn on commit, warn on build, error on push).

use crate::changelist_helpers::{self, ClKind, ClStatus, ParsedChangelist};
use crate::type_scanner;
use crate::{CrossCrateLint, LintContext, LintError};

const LINT_NAME: &str = "deprecation-comparison";

pub struct DeprecationComparison;

impl CrossCrateLint for DeprecationComparison {
    fn default_severity(&self) -> crate::Severity { crate::Severity::PUSH_GATE }
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn source_only(&self) -> bool {
        false
    }

    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        let workspace_root = match crates.first() {
            Some((_, ctx)) => ctx.workspace_root,
            None => return Vec::new(),
        };

        let design_rounds = workspace_root.join("design_rounds");
        let all_cls = changelist_helpers::find_changelists(&design_rounds);

        let deprecated: Vec<&ParsedChangelist> = all_cls
            .iter()
            .filter(|cl| cl.status == ClStatus::Deprecated)
            .collect();

        if deprecated.is_empty() {
            return Vec::new();
        }

        // Collect SHAME.md.tmpl contents from every crate so the escape
        // hatch is uniform: a sufficient SHAME entry keyed by either the
        // active CL filename (`## 202604191900_changelist.doc.lock.md`)
        // or the generic header `## deprecation-comparison` waives the
        // error for the CL in question.
        let shame_blobs: Vec<&str> = crates
            .iter()
            .filter_map(|(_, ctx)| ctx.shame_doc)
            .collect();

        let mut errors = Vec::new();

        for dep_cl in &deprecated {
            // Find the active (non-deprecated, non-locked) CL of the same kind.
            // Also accept locked CLs — they should have had the comparison when active.
            let active_cl = all_cls.iter().find(|cl| {
                cl.kind == dep_cl.kind
                    && cl.status != ClStatus::Deprecated
            });

            let active_cl = match active_cl {
                Some(cl) => cl,
                None => continue, // no active CL of same kind — not a violation
            };

            // SHAME check — keyed by active CL filename.
            if shame_entry_with_min_words(&shame_blobs, &active_cl.filename, 50) {
                continue;
            }

            // Read both files
            let dep_path = design_rounds.join(&dep_cl.filename);
            let active_path = design_rounds.join(&active_cl.filename);

            let dep_content = match std::fs::read_to_string(&dep_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let active_content = match std::fs::read_to_string(&active_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            errors.extend(check_comparison(
                &dep_cl.filename,
                &dep_content,
                &active_cl.filename,
                &active_content,
                dep_cl.kind,
            ));
        }

        errors
    }
}

/// True if any of the SHAME blobs contains a `## <key>` entry with at
/// least `min_words` of body text after the header and before the next
/// `## ` header. Matches the SHAME format used by `undocumented_type` and
/// `design_doc_source_mismatch`.
fn shame_entry_with_min_words(shame_blobs: &[&str], key: &str, min_words: usize) -> bool {
    let header = format!("## {key}");
    for blob in shame_blobs {
        if let Some(start) = blob.find(&header) {
            let after = &blob[start + header.len()..];
            let end = after.find("\n## ").unwrap_or(after.len());
            let body = after[..end].trim();
            if body.split_whitespace().count() >= min_words {
                return true;
            }
        }
    }
    false
}

/// Check that the active CL has a proper comparison section for the deprecated CL.
fn check_comparison(
    dep_filename: &str,
    dep_content: &str,
    active_filename: &str,
    active_content: &str,
    kind: ClKind,
) -> Vec<LintError> {
    let mut errors = Vec::new();
    let kind_label = match kind {
        ClKind::Doc => "doc",
        ClKind::Src => "src",
    };

    // 1. Check for comparison section header (case-insensitive)
    let comparison_section = find_comparison_section(active_content);
    if comparison_section.is_none() {
        errors.push(LintError::push_error(
            "workspace".to_string(),
            0,
            LINT_NAME,
            format!(
                "{kind_label} changelist `{active_filename}` replaces deprecated \
                 `{dep_filename}` but is missing a `## Comparison to deprecated changelist` \
                 section. Add this section to explain what changed from the deprecated version.",
            ),
        ));
        return errors;
    }

    let comparison_text = comparison_section.unwrap();

    // 2. Check that each ## header from deprecated CL has a matching ### subheader
    let dep_headers = extract_h2_headers(dep_content);
    let comparison_subheaders = extract_h3_headers_lowercase(&comparison_text);

    for header in &dep_headers {
        let header_lower = header.to_lowercase();
        if !comparison_subheaders.iter().any(|h| h == &header_lower) {
            errors.push(LintError::push_error(
                "workspace".to_string(),
                0,
                LINT_NAME,
                format!(
                    "{kind_label} changelist `{active_filename}`: comparison section \
                     is missing a `### {header}` subheader corresponding to `## {header}` \
                     in deprecated `{dep_filename}`.",
                ),
            ));
        }
    }

    // 3. Check that all backtick-wrapped type names from deprecated CL appear
    //    in the comparison section
    let dep_names = type_scanner::extract_backtick_names(dep_content);
    for name in &dep_names {
        if !comparison_text.contains(name) {
            errors.push(LintError::push_error(
                "workspace".to_string(),
                0,
                LINT_NAME,
                format!(
                    "{kind_label} changelist `{active_filename}`: comparison section \
                     does not mention `{name}` which appears in deprecated `{dep_filename}`. \
                     Address all types/traits/macros from the deprecated changelist.",
                ),
            ));
        }
    }

    errors
}

/// Find the comparison section and return its full text (from the header to the
/// next `##` header or end of file).
fn find_comparison_section(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ")
            && trimmed[3..].trim().to_lowercase().starts_with("comparison to deprecated changelist")
        {
            start = Some(i);
            break;
        }
    }

    let start = start?;

    // Find the end: next ## header or end of file
    let mut end = lines.len();
    for i in (start + 1)..lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
            end = i;
            break;
        }
    }

    Some(lines[start..end].join("\n"))
}

/// Extract all `## Header` text from markdown (the header titles, not the `##` prefix).
fn extract_h2_headers(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") && !trimmed.starts_with("### ") {
                Some(trimmed[3..].trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Extract all `### Header` titles from text, lowercased for comparison.
fn extract_h3_headers_lowercase(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("### ") {
                Some(trimmed[4..].trim().to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cl(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    // -----------------------------------------------------------------------
    // find_comparison_section tests
    // -----------------------------------------------------------------------

    #[test]
    fn finds_comparison_section() {
        let content = "\
## Overview
Some overview.

## Comparison to deprecated changelist

### Overview
Changes here.

## Next section
";
        let section = find_comparison_section(content).unwrap();
        assert!(section.contains("### Overview"));
        assert!(section.contains("Changes here."));
        assert!(!section.contains("## Next section"));
    }

    #[test]
    fn finds_comparison_section_case_insensitive() {
        let content = "## comparison to Deprecated Changelist\nSome text.\n";
        assert!(find_comparison_section(content).is_some());
    }

    #[test]
    fn no_comparison_section() {
        let content = "## Overview\nJust overview.\n";
        assert!(find_comparison_section(content).is_none());
    }

    #[test]
    fn comparison_section_at_end_of_file() {
        let content = "\
## Comparison to deprecated changelist

### Types
All types covered.
";
        let section = find_comparison_section(content).unwrap();
        assert!(section.contains("### Types"));
        assert!(section.contains("All types covered."));
    }

    // -----------------------------------------------------------------------
    // extract_h2_headers tests
    // -----------------------------------------------------------------------

    #[test]
    fn extracts_h2_headers() {
        let content = "\
## Overview
text
## Storage types
text
### Not this one
## Final notes
";
        let headers = extract_h2_headers(content);
        assert_eq!(headers, vec!["Overview", "Storage types", "Final notes"]);
    }

    // -----------------------------------------------------------------------
    // extract_h3_headers_lowercase tests
    // -----------------------------------------------------------------------

    #[test]
    fn extracts_h3_lowercase() {
        let content = "\
### Overview
### Storage Types
### final notes
";
        let headers = extract_h3_headers_lowercase(content);
        assert_eq!(headers, vec!["overview", "storage types", "final notes"]);
    }

    // -----------------------------------------------------------------------
    // check_comparison tests
    // -----------------------------------------------------------------------

    #[test]
    fn passes_when_comparison_is_complete() {
        let deprecated = "\
## Overview
Uses `StorageRecord` and `Behavior`.
## Types
The `Marker` type.
";
        let active = "\
## Changes
New approach.

## Comparison to deprecated changelist

### Overview
Changed approach to `StorageRecord` and `Behavior`.

### Types
The `Marker` type was redesigned.
";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn fails_when_comparison_section_missing() {
        let deprecated = "## Overview\nSome text.\n";
        let active = "## Changes\nNew approach.\n";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("missing a `## Comparison to deprecated changelist`"));
    }

    #[test]
    fn fails_when_subheader_missing() {
        let deprecated = "\
## Overview
Text.
## Types
More text.
";
        let active = "\
## Comparison to deprecated changelist

### Overview
Covered.
";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        // Missing ### Types
        assert!(errors.iter().any(|e| e.message.contains("### Types")));
    }

    #[test]
    fn fails_when_type_name_missing() {
        let deprecated = "\
## Overview
Uses `StorageRecord` and `Behavior`.
";
        let active = "\
## Comparison to deprecated changelist

### Overview
Only mentions `StorageRecord`.
";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        assert!(errors.iter().any(|e| e.message.contains("`Behavior`")));
    }

    #[test]
    fn subheader_matching_is_case_insensitive() {
        let deprecated = "\
## Storage Types
Text about `Foo`.
";
        let active = "\
## Comparison to deprecated changelist

### storage types
Mentions `Foo`.
";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn src_kind_label_in_messages() {
        let deprecated = "## Overview\nText.\n";
        let active = "## Changes\nNew.\n";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.src.md",
            active,
            ClKind::Src,
        );
        assert!(errors[0].message.starts_with("src changelist"));
    }

    // -----------------------------------------------------------------------
    // Integration test with filesystem
    // -----------------------------------------------------------------------

    #[test]
    fn integration_no_deprecated_no_errors() {
        let tmp = TempDir::new().unwrap();
        let dr = tmp.path().join("design_rounds");
        fs::create_dir_all(&dr).unwrap();
        write_cl(&dr, "202603071430_changelist.doc.md", "## Overview\nText.\n");

        let cls = changelist_helpers::find_changelists(&dr);
        let deprecated: Vec<_> = cls.iter().filter(|c| c.status == ClStatus::Deprecated).collect();
        assert!(deprecated.is_empty());
    }

    #[test]
    fn integration_deprecated_with_active_missing_comparison() {
        let tmp = TempDir::new().unwrap();
        let dr = tmp.path().join("design_rounds");
        fs::create_dir_all(&dr).unwrap();

        write_cl(
            &dr,
            "202603071430_changelist.doc.deprecated.md",
            "## Overview\nUses `StorageRecord`.\n",
        );
        write_cl(
            &dr,
            "202603081000_changelist.doc.md",
            "## New approach\nDifferent design.\n",
        );

        let all_cls = changelist_helpers::find_changelists(&dr);
        let deprecated: Vec<&ParsedChangelist> = all_cls
            .iter()
            .filter(|cl| cl.status == ClStatus::Deprecated)
            .collect();

        assert_eq!(deprecated.len(), 1);

        // Simulate the lint check
        let dep_cl = deprecated[0];
        let active_cl = all_cls
            .iter()
            .find(|cl| cl.kind == dep_cl.kind && cl.status != ClStatus::Deprecated)
            .unwrap();

        let dep_content = fs::read_to_string(dr.join(&dep_cl.filename)).unwrap();
        let active_content = fs::read_to_string(dr.join(&active_cl.filename)).unwrap();

        let errors = check_comparison(
            &dep_cl.filename,
            &dep_content,
            &active_cl.filename,
            &active_content,
            dep_cl.kind,
        );

        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Comparison to deprecated changelist"));
    }

    #[test]
    fn integration_deprecated_with_complete_comparison() {
        let tmp = TempDir::new().unwrap();
        let dr = tmp.path().join("design_rounds");
        fs::create_dir_all(&dr).unwrap();

        write_cl(
            &dr,
            "202603071430_changelist.doc.deprecated.md",
            "## Overview\nUses `StorageRecord`.\n## Types\n`Marker` type.\n",
        );
        write_cl(
            &dr,
            "202603081000_changelist.doc.md",
            "\
## New approach
Different design.

## Comparison to deprecated changelist

### Overview
`StorageRecord` redesigned.

### Types
`Marker` type kept.
",
        );

        let all_cls = changelist_helpers::find_changelists(&dr);
        let dep_cl = all_cls
            .iter()
            .find(|cl| cl.status == ClStatus::Deprecated)
            .unwrap();
        let active_cl = all_cls
            .iter()
            .find(|cl| cl.kind == dep_cl.kind && cl.status != ClStatus::Deprecated)
            .unwrap();

        let dep_content = fs::read_to_string(dr.join(&dep_cl.filename)).unwrap();
        let active_content = fs::read_to_string(dr.join(&active_cl.filename)).unwrap();

        let errors = check_comparison(
            &dep_cl.filename,
            &dep_content,
            &active_cl.filename,
            &active_content,
            dep_cl.kind,
        );

        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn integration_deprecated_without_active_no_error() {
        let tmp = TempDir::new().unwrap();
        let dr = tmp.path().join("design_rounds");
        fs::create_dir_all(&dr).unwrap();

        // Only a deprecated CL, no active replacement
        write_cl(
            &dr,
            "202603071430_changelist.doc.deprecated.md",
            "## Overview\nOld stuff.\n",
        );

        let all_cls = changelist_helpers::find_changelists(&dr);
        let deprecated: Vec<_> = all_cls
            .iter()
            .filter(|cl| cl.status == ClStatus::Deprecated)
            .collect();

        assert_eq!(deprecated.len(), 1);

        // No active CL of same kind means no violation
        let active = all_cls
            .iter()
            .find(|cl| cl.kind == deprecated[0].kind && cl.status != ClStatus::Deprecated);
        assert!(active.is_none());
    }

    #[test]
    fn severity_is_push_gate() {
        let deprecated = "## Overview\nText.\n";
        let active = "## Changes\nNew.\n";
        let errors = check_comparison(
            "old.deprecated.md",
            deprecated,
            "new.doc.md",
            active,
            ClKind::Doc,
        );
        assert!(!errors.is_empty());
        assert_eq!(errors[0].severity, crate::Severity::PUSH_GATE);
    }
}
