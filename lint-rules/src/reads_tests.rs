//! A repo lint declaring the paths it reads is run by a commit touching them, and only by one.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Lint, LintError, Reach, RepoContext, RepoLint, Touched, check_repo_with_extra};

/// Reads `content/**` and has a finding every time it runs, counting the runs.
struct Assets {
    runs: Arc<AtomicUsize>,
}

impl Lint for Assets {
    fn name(&self) -> &'static str {
        "reads-assets"
    }

    fn reach(&self) -> Reach {
        Reach::Paths(&["content/**"])
    }
}

impl RepoLint for Assets {
    fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        vec![LintError::error("alpha".to_string(), 0, "reads-assets", "broken".to_string())]
    }
}

/// Declares nothing, so it reads anything.
struct Silent {
    runs: Arc<AtomicUsize>,
}

impl Lint for Silent {
    fn name(&self) -> &'static str {
        "reads-nothing-declared"
    }
}

impl RepoLint for Silent {
    fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    }
}

/// How often each lint ran, and what they found, for a run changing `touched`.
fn run(touched: &Touched) -> (usize, usize, Vec<LintError>) {
    let (assets, silent) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let extra: Vec<Box<dyn RepoLint>> = vec![
        Box::new(Assets { runs: Arc::clone(&assets) }),
        Box::new(Silent { runs: Arc::clone(&silent) }),
    ];
    let (crates, registry) = (BTreeSet::new(), Default::default());
    let ctx = RepoContext {
        mock_dir: std::path::Path::new("/mock"),
        repo_root: std::path::Path::new("/"),
        all_crates: &crates,
        src_dirs: &[],
        invocation: None,
        canon_paths: &[],
        open_panels: &[],
        registry: &registry,
        verdicts: None,
    };
    let found = check_repo_with_extra(&ctx, false, None, &extra, touched)
        .into_iter()
        .filter(|e| e.lint_name == "reads-assets")
        .collect();
    (assets.load(Ordering::SeqCst), silent.load(Ordering::SeqCst), found)
}

#[test]
fn a_commit_touching_what_the_lint_reads_runs_it_and_its_finding_stays() {
    let changed = vec!["content/packs/a/manifest.toml".to_string()];
    let (assets, _, found) = run(&Touched::Files(&changed));
    assert_eq!(assets, 1);
    assert_eq!(found.len(), 1, "the finding of a lint that ran must reach the gate");
}

#[test]
fn a_commit_touching_none_of_it_skips_the_lint_and_not_the_undeclared_one() {
    let changed = vec!["mock/crates/kaski-engine-hash/src/lib.rs".to_string()];
    let (assets, silent, found) = run(&Touched::Files(&changed));
    assert_eq!((assets, silent), (0, 1));
    assert!(found.is_empty());
}

#[test]
fn a_run_judged_whole_runs_both() {
    let (assets, silent, found) = run(&Touched::Everything);
    assert_eq!((assets, silent, found.len()), (1, 1, 1));
}
