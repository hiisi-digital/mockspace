//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Every bench result and every history record lives where the harness writes
//! it, in the one layout.
//!
//! The harness writes each run once, into
//! `results/<bench>/<sweep>_n<point>/<machine class>/<run>/`, so runs over time
//! and across machines sit side by side and a cell's history is read across
//! them. A result anywhere else is invisible to that reading: the index, the
//! regression check and the docs pass it by, and it cannot be compared with
//! anything. So a bench output outside the layout is refused, whichever older
//! convention wrote it.
//!
//! The grammar is `mockspace_bench_layout::place`, the same one the harness
//! writes through and `mock bench migrate` translates through. Where it calls
//! a file a translatable old layout, the finding names `mock bench migrate`;
//! where nothing translates it one to one, the finding says to move it by hand.

use crate::{Entry, Lint, LintError, RepoContext, RepoLint, Severity};
use mockspace_bench_layout::{Placement, RESULTS_DIR, place};

const NAME: &str = "bench-results-live-in-the-harness-layout";

/// The lint.
pub struct BenchResultsLiveInTheHarnessLayout;

impl Lint for BenchResultsLiveInTheHarnessLayout {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "every bench result and history record sits in a run directory of the harness's one layout"
    }

    fn source_only(&self) -> bool {
        false
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl RepoLint for BenchResultsLiveInTheHarnessLayout {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let view = ctx.view();
        let bench_dir = ctx.mock_dir.join("benches");
        if !view.is_dir(&bench_dir) {
            return Vec::new();
        }
        let results = bench_dir.join(RESULTS_DIR);
        // A run in progress and a voided run are the harness's own, and
        // nothing under them is read; `target/` and `.git` are not bench data.
        let files = view.walk(&bench_dir, |path, e| {
            Entry::is_dir(e)
                && (matches!(e.name(), "target" | ".git")
                    || path == results.join(mockspace_bench_layout::INFLIGHT_DIR)
                    || path == results.join(mockspace_bench_layout::VOID_DIR))
        });
        let tree = bench_dir
            .strip_prefix(ctx.repo_root)
            .unwrap_or(&bench_dir)
            .display()
            .to_string();
        let mut out = Vec::new();
        for file in files {
            let Ok(rel) = file.strip_prefix(&bench_dir) else {
                continue;
            };
            let message = match place(rel) {
                Placement::NotOutput | Placement::InLayout => continue,
                Placement::Legacy(l) => {
                    format!(
                        "{} is a bench output in an older layout. Run `mock bench migrate`, which \
                         moves it to `{RESULTS_DIR}/{}/{}/<machine class>/<run>/{}` and commits \
                         nothing; review and commit the move.",
                        rel.display(),
                        l.bench,
                        l.cell.name(),
                        l.part.run_file()
                    )
                },
                Placement::Stray(s) => {
                    format!(
                        "{} {}, so no reading of the bench's history ever sees it. Nothing \
                         translates it one to one: move it by hand into the run directory it \
                         belongs to, `{RESULTS_DIR}/<bench>/<sweep>_n<point>/<machine class>/<run>/`, \
                         or delete it if it is not a bench result.",
                        rel.display(),
                        s.says()
                    )
                },
            };
            let mut e = LintError::error(tree.clone(), 0, NAME, message);
            e.path = Some(rel.display().to_string());
            out.push(e);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::RepoInput;

    fn errors(files: &[&str]) -> Vec<LintError> {
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        for f in files {
            let p = mock.join("benches").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir_all(&mock).unwrap();
        let registry = crate::RegistryView::default();
        let no_crates = BTreeSet::new();
        let input = RepoInput {
            mock_dir:    &mock,
            repo_root:   tmp.path(),
            all_crates:  &no_crates,
            src_dirs:    &[],
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            ref_files:   &crate::src_layout::NO_REF_FILES,
            registry:    &registry,
        };
        crate::testkit::check_repo(&BenchResultsLiveInTheHarnessLayout, input)
    }

    fn findings(files: &[&str]) -> Vec<String> {
        errors(files).into_iter().map(|e| e.path.unwrap_or_default()).collect()
    }

    const RUN: &str = "results/hash/hash_n64/linux-x86_64-c4-bare-amd/20261115T093000Z-a";

    /// A tree the harness wrote, with its sources, passes.
    #[test]
    fn a_tree_in_the_layout_passes() {
        let files = [
            "bench.toml".to_string(),
            "hash/arms/fnv/src/lib.rs".to_string(),
            format!("{RUN}/samples.csv"),
            format!("{RUN}/meta.json"),
            format!("{RUN}/report.md"),
            format!("{RUN}/summary.tsv"),
            "results/INDEX.md".to_string(),
            "results/void/1-2/hash/hash_n64.csv".to_string(),
            "target/release/x_n64.csv".to_string(),
        ];
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        assert_eq!(findings(&refs), Vec::<String>::new());
    }

    /// Every older layout is refused, the translatable ones and the strays,
    /// each by its own path: hilavitkutin's root files and both ledgers, the
    /// harness's flat results, and the outputs a runner of its own left in a
    /// member and in a directory of runs.
    #[test]
    fn every_older_layout_is_refused() {
        let found = findings(&[
            "atomic_inc_n1024.csv",
            "atomic_inc_n1024.meta.json",
            "atomic_inc_n1024_findings.md",
            ".bench_history/atomic_inc_n1024.tsv",
            "history/warm/width-l1_n80003.tsv",
            "results/hash/hash_n64.csv",
            "runs/run1/rsb_n256.csv",
            "frame_wide/heavy_n65536.meta.json",
            "results/hash/notes.txt",
        ]);
        assert_eq!(found, [
            ".bench_history/atomic_inc_n1024.tsv",
            "atomic_inc_n1024.csv",
            "atomic_inc_n1024.meta.json",
            "atomic_inc_n1024_findings.md",
            "frame_wide/heavy_n65536.meta.json",
            "history/warm/width-l1_n80003.tsv",
            "results/hash/hash_n64.csv",
            "results/hash/notes.txt",
            "runs/run1/rsb_n256.csv",
        ]);
    }

    #[test]
    fn a_translatable_file_names_the_migration_and_a_stray_says_to_move_it() {
        let tmp_msgs = |f: &str| errors(&[f]).into_iter().map(|e| e.message).collect::<Vec<_>>();
        let m = tmp_msgs("results/hash/hash_n64_findings.md");
        assert!(m[0].contains("mock bench migrate"), "{m:?}");
        assert!(m[0].contains("results/hash/hash_n64/<machine class>/<run>/report.md"), "{m:?}");
        let m = tmp_msgs("runs/run1/rsb_n256.csv");
        assert!(m[0].contains("by hand"), "{m:?}");
        assert!(!m[0].contains("mock bench migrate"), "{m:?}");
    }
}
