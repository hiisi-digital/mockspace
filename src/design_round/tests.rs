//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::design_round::*;

    #[test]
    fn disambiguate_returns_base_when_free() {
        let got = disambiguate_archive_name("202606072000", |_| false);
        assert_eq!(got, "202606072000", "a free name is used as-is");
    }

    #[test]
    fn disambiguate_appends_suffix_on_collision() {
        // Only the base name collides; `-2` is free.
        let got = disambiguate_archive_name("202606072000", |n| n == "202606072000");
        assert_eq!(got, "202606072000-2", "first collision bumps to -2");
    }

    #[test]
    fn disambiguate_walks_past_multiple_collisions() {
        // base, -2, and -3 all taken; -4 is the first free name.
        let taken = ["202606072000", "202606072000-2", "202606072000-3"];
        let got = disambiguate_archive_name("202606072000", |n| taken.contains(&n));
        assert_eq!(got, "202606072000-4", "walks past every taken name");
    }

    #[test]
    fn rewrite_new_format_doc_to_locked() {
        let result = rewrite_filename(
            "202603071430_changelist.doc.md",
            ClKind::Doc,
            ClStatus::Locked,
        );
        assert_eq!(result.unwrap(), "202603071430_changelist.doc.lock.md");
    }

    #[test]
    fn rewrite_new_format_src_to_deprecated() {
        let result = rewrite_filename(
            "202603071430_changelist.src.md",
            ClKind::Src,
            ClStatus::Deprecated,
        );
        assert_eq!(result.unwrap(), "202603071430_changelist.src.deprecated.md");
    }

    #[test]
    fn rewrite_locked_to_active() {
        let result = rewrite_filename(
            "202603071430_changelist.doc.lock.md",
            ClKind::Doc,
            ClStatus::Active,
        );
        assert_eq!(result.unwrap(), "202603071430_changelist.doc.md");
    }

    #[test]
    fn round_name_from_new_format() {
        let cls = vec![
            ParsedChangelist {
                filename: "202603101430_changelist.doc.lock.md".to_string(),
                kind:     ClKind::Doc,
                status:   ClStatus::Locked,
            },
            ParsedChangelist {
                filename: "202603101500_changelist.src.lock.md".to_string(),
                kind:     ClKind::Src,
                status:   ClStatus::Locked,
            },
        ];
        assert_eq!(determine_round_name(&cls), "202603101430");
    }

    #[test]
    fn round_name_from_legacy() {
        let cls = vec![ParsedChangelist {
            filename: "2026-03-07_changelist.lock.md".to_string(),
            kind:     ClKind::Doc,
            status:   ClStatus::Locked,
        }];
        assert_eq!(determine_round_name(&cls), "2026-03-07");
    }

    // --- migrate tests ---

    #[test]
    fn detect_legacy_filename() {
        assert!(is_legacy_filename("2026-03-07_corrections.md"));
        assert!(is_legacy_filename(
            "2026-03-06_source-doc-divergence-audit.md"
        ));
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
        assert_eq!(
            result.unwrap(),
            "202603060000_topic.source-doc-divergence-audit.md"
        );
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
        // notes, dotfiles produced elsewhere: none should affect naming.
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

    // --- tagged round ids ---

    #[test]
    fn rewrite_keeps_the_tag_of_a_tagged_changelist() {
        // Mutant: `rewrite_filename` goes back to twelve digits.
        let result = rewrite_filename(
            "202610221529-kx3f_changelist.doc.md",
            ClKind::Doc,
            ClStatus::Locked,
        );
        assert_eq!(result.unwrap(), "202610221529-kx3f_changelist.doc.lock.md");
    }

    #[test]
    fn round_name_of_a_tagged_round_carries_the_tag() {
        // Mutant: `determine_round_name` cuts the id to twelve characters, so a
        // tagged round archives under the bare minute and collides again.
        let cls = vec![
            ParsedChangelist {
                filename: "202610221529-kx3f_changelist.doc.lock.md".to_string(),
                kind:     ClKind::Doc,
                status:   ClStatus::Locked,
            },
            ParsedChangelist {
                filename: "202610221540-kx3f_changelist.src.lock.md".to_string(),
                kind:     ClKind::Src,
                status:   ClStatus::Locked,
            },
        ];
        assert_eq!(determine_round_name(&cls), "202610221529-kx3f");
    }

    #[test]
    fn round_name_from_dir_reads_old_and_new_ids_in_time_order() {
        // Mutant: `determine_round_name_from_dir` reads twelve characters, or
        // sorts the tag ahead of the minute.
        let tmp = tempfile::tempdir().expect("tempdir");
        let dr = tmp.path();
        std::fs::write(dr.join("202610221600_topic.old.md"), "x").unwrap();
        std::fs::write(dr.join("202610221530-zzzz_topic.new.md"), "x").unwrap();
        std::fs::write(dr.join("202610221545-aaaa_topic.newer.md"), "x").unwrap();
        assert_eq!(
            determine_round_name_from_dir(dr).as_deref(),
            Some("202610221530-zzzz")
        );
    }

    #[test]
    fn two_rounds_in_one_minute_get_two_names_without_the_count() {
        // Mutant: `fresh` ignores `taken`, so the second round of a minute gets
        // the first one's tag and `disambiguate_archive_name` is the only thing
        // left between them.
        let first = lint_rules::round_id::fresh("202610221529", "a", |_| false);
        let second = lint_rules::round_id::fresh("202610221529", "b", |_| false);
        assert_ne!(first, second);
        assert_eq!(disambiguate_archive_name(&first, |n| n == second), first);
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
            "chore: archive abandoned round 202604191100-abandoned",
        );
    }
}

#[cfg(test)]
mod seal;

#[cfg(test)]
mod close_scope;
