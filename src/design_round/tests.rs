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

/// `lock` and `close` refusing an unsealed round themselves, which is the only
/// guard a hook-less `--auto-commit` passes through.
#[cfg(test)]
mod seal {
    use std::path::Path;
    use std::process::ExitCode;

    use crate::config::Config;
    use crate::design_round::*;

    const OPTS: SubcmdOpts = SubcmdOpts {
        auto_commit: false,
    };

    fn mock(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("design_rounds")).unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join("design_rounds").join(name), text).unwrap();
        }
        dir
    }

    fn listed(mock: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(mock.join("design_rounds"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn lock_refuses_a_bare_title_in_either_window_and_moves_nothing() {
        let cases: [&[(&str, &str)]; 2] =
            [&[("202609181543_changelist.doc.md", "# doc changelist\n\n")], &[
                (
                    "202609181543_changelist.doc.lock.md",
                    "# doc changelist\n\nNone.\n",
                ),
                ("202609181544_changelist.src.md", "# src changelist\n\n"),
            ]];
        for files in cases {
            let dir = mock(files);
            let before = listed(dir.path());
            let code = cmd_lock(&Config::from_dir(dir.path()), &OPTS);
            assert_eq!(code, ExitCode::FAILURE, "{files:?}");
            assert_eq!(listed(dir.path()), before, "{files:?}");
        }
    }

    #[test]
    fn lock_locks_a_changelist_that_says_something() {
        let dir = mock(&[("202609181543_changelist.doc.md", "# doc changelist: none\n")]);
        let code = cmd_lock(&Config::from_dir(dir.path()), &OPTS);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(listed(dir.path()), ["202609181543_changelist.doc.lock.md"]);
    }

    #[test]
    fn close_refuses_an_empty_lock_and_moves_nothing() {
        // CLOSED by phase, with the src lock a bare template: the state an
        // auto-committed close would otherwise carry into history unread.
        let dir = mock(&[
            (
                "202609181236_changelist.doc.lock.md",
                "# doc changelist\n\nNone.\n",
            ),
            (
                "202609181237_changelist.src.lock.md",
                "# src changelist\n\n",
            ),
        ]);
        let before = listed(dir.path());
        let code = cmd_close(&Config::from_dir(dir.path()), &OPTS);
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(listed(dir.path()), before);
    }

    // --- a doc lock and the templates it freezes ------------------------------

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed in {}", dir.display());
    }

    const DOC_CL: &str = "202609190100_changelist.doc.md";

    /// A repository in DOC, with a doc changelist that says something and one
    /// crate template, everything committed. Returns the repo and its mock dir.
    fn repo_in_doc() -> (tempfile::TempDir, std::path::PathBuf) {
        let repo = tempfile::tempdir().unwrap();
        let mock = repo.path().join("mock");
        std::fs::create_dir_all(mock.join("design_rounds")).unwrap();
        std::fs::create_dir_all(mock.join("crates/x")).unwrap();
        std::fs::write(mock.join("design_rounds").join(DOC_CL), "# doc changelist: x\n").unwrap();
        std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x\n").unwrap();
        std::fs::write(mock.join("crates/x/SHAME.md.tmpl"), "# shame\n").unwrap();
        std::fs::write(mock.join("README.md.tmpl"), "# readme\n").unwrap();
        git(repo.path(), &["init", "-q", "-b", "dev"]);
        git(repo.path(), &["config", "user.name", "t"]);
        git(repo.path(), &["config", "user.email", "t@example.com"]);
        git(repo.path(), &["config", "commit.gpgsign", "false"]);
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "--no-verify", "-m", "chore: a round"]);
        (repo, mock)
    }

    fn lock_in(mock: &Path) -> (ExitCode, Vec<String>, Vec<String>) {
        let before = listed(mock);
        let code = cmd_lock(&Config::from_dir(mock), &OPTS);
        (code, before, listed(mock))
    }

    #[test]
    fn a_doc_lock_goes_through_when_every_template_is_committed() {
        // The control. Without it every refusal below passes on a lock that
        // refuses whatever it is handed.
        let (_repo, mock) = repo_in_doc();
        let (code, _, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(after, ["202609190100_changelist.doc.lock.md"]);
    }

    #[test]
    fn a_doc_lock_is_refused_over_a_template_edited_and_not_committed() {
        let (_repo, mock) = repo_in_doc();
        std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, edited\n").unwrap();
        let (code, before, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(after, before, "a refused lock moved a file");
    }

    #[test]
    fn a_doc_lock_is_refused_over_a_template_staged_and_not_committed() {
        let (repo, mock) = repo_in_doc();
        std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, staged\n").unwrap();
        git(repo.path(), &["add", "mock/crates/x/DESIGN.md.tmpl"]);
        let (code, before, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(after, before);
    }

    #[test]
    fn a_doc_lock_is_refused_over_a_new_template_nobody_added() {
        let (_repo, mock) = repo_in_doc();
        std::fs::write(mock.join("crates/x/DEEPDIVE_new.md.tmpl"), "# new\n").unwrap();
        let (code, before, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(after, before);
    }

    #[test]
    fn what_the_doc_gate_leaves_open_does_not_hold_the_lock() {
        // SHAME is writable in every phase, and a template outside the source
        // directories is not gated at all, so neither can be stranded.
        let (_repo, mock) = repo_in_doc();
        std::fs::write(mock.join("crates/x/SHAME.md.tmpl"), "# shame, edited\n").unwrap();
        std::fs::write(mock.join("README.md.tmpl"), "# readme, edited\n").unwrap();
        let (code, _, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::SUCCESS);
        assert_eq!(after, ["202609190100_changelist.doc.lock.md"]);
    }

    #[test]
    fn a_src_lock_does_not_ask_about_templates() {
        // A dirty template in IMPL is the doc gate's to refuse at commit; the
        // src lock ends a different window and is not where it is caught.
        let (repo, mock) = repo_in_doc();
        let dr = mock.join("design_rounds");
        std::fs::rename(dr.join(DOC_CL), dr.join("202609190100_changelist.doc.lock.md")).unwrap();
        std::fs::write(dr.join("202609190101_changelist.src.md"), "# src changelist: x\n")
            .unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "--no-verify", "-m", "chore: into impl"]);
        std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, edited\n").unwrap();
        let (code, _, after) = lock_in(&mock);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(after.contains(&"202609190101_changelist.src.lock.md".to_string()), "{after:?}");
    }

    /// homma's shape itself: the text unlocked beside a lock of its kind.
    /// Refused whichever check reaches it first, the phase or the seal, and
    /// what matters is that nothing moves.
    #[test]
    fn close_refuses_the_homma_shape_and_moves_nothing() {
        let dir = mock(&[
            (
                "202609181236_changelist.doc.lock.md",
                "# doc changelist\n\nNone.\n",
            ),
            (
                "202609181236_changelist.src.md",
                "# src changelist: x\n\n## CHANGE: y\n",
            ),
            (
                "202609181237_changelist.src.lock.md",
                "# src changelist\n\n",
            ),
        ]);
        let before = listed(dir.path());
        let code = cmd_close(&Config::from_dir(dir.path()), &OPTS);
        assert_eq!(code, ExitCode::FAILURE);
        assert_eq!(listed(dir.path()), before);
    }
}
