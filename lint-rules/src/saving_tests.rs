//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A run answered from saved verdicts gives the findings a full run gives, and recomputes only
//! what moved.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::testkit::LintFixture;
use crate::verdict::{self, VerdictCache};
use crate::{
    CrateLint, Lint, LintConfig, LintContext, LintError, Reach, Severity, check_crate_saving,
    check_crate_with_extra,
};

/// An in-memory store that counts what is asked of it.
#[derive(Default)]
struct Memory {
    saved:  RefCell<HashMap<String, String>>,
    hits:   AtomicUsize,
    stores: AtomicUsize,
}

impl VerdictCache for Memory {
    fn digest(&self, parts: &[&[u8]]) -> String {
        let mut out = String::new();
        for p in parts {
            out.push_str(&format!("{}:", p.len()));
            out.push_str(&String::from_utf8_lossy(p));
            out.push('|');
        }
        out
    }

    fn replay(&self, key: &str) -> Option<String> {
        let found = self.saved.borrow().get(key).cloned();
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::SeqCst);
        }
        found
    }

    fn remember(&self, key: &str, encoded: &str) {
        self.stores.fetch_add(1, Ordering::SeqCst);
        self.saved.borrow_mut().insert(key.to_string(), encoded.to_string());
    }
}

/// A lint that finds `FIXME` lines and counts how often it actually runs.
struct Counting {
    reach: Reach,
    runs:  Arc<AtomicUsize>,
}

impl Counting {
    /// The lint, boxed for the runner, and the counter the test keeps.
    fn boxed(reach: Reach) -> (Vec<Box<dyn CrateLint>>, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        (vec![Box::new(Self { reach, runs: Arc::clone(&runs) })], runs)
    }
}

impl Lint for Counting {
    fn name(&self) -> &'static str {
        "counting"
    }

    fn reach(&self) -> Reach {
        self.reach
    }

    fn per_file(&self) -> bool {
        self.reach != Reach::Crate
    }
}

impl CrateLint for Counting {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let mut found = Vec::new();
        for (i, line) in ctx.source.lines().enumerate() {
            if line.contains("FIXME") {
                found.push(LintError::error(
                    ctx.crate_name.to_string(),
                    i + 1,
                    "counting",
                    "fixme".into(),
                ));
            }
        }
        found
    }
}

fn crate_of(root: &str, module: &str) -> LintFixture {
    LintFixture::new(root).with_module("src/m.rs", module)
}

/// Findings as a sorted list of lines, so two runs are compared as the sets they are.
fn lines(found: &[LintError]) -> Vec<String> {
    let mut l: Vec<String> = verdict::encode(found).lines().skip(1).map(String::from).collect();
    l.sort();
    l
}

const ROOT: &str = "pub fn a() { todo!() }\n// FIXME: root\n";
const MODULE: &str = "pub fn m() { todo!() }\n";

#[test]
fn a_saved_run_gives_the_findings_a_full_run_gives() {
    let fixture = crate_of(ROOT, MODULE);
    let ctx = fixture.ctx();
    let full = check_crate_with_extra(&ctx, false, None, &[]);
    assert!(!full.is_empty(), "the control: the crate has findings to save");

    let store = Memory::default();
    let cold = check_crate_saving(&ctx, false, None, &[], Some(&store));
    assert_eq!(lines(&cold), lines(&full), "a cold saved run differs from a full run");
    assert!(store.stores.load(Ordering::SeqCst) > 0, "a cold run must save what it judged");

    let stores_after_cold = store.stores.load(Ordering::SeqCst);
    let warm = check_crate_saving(&ctx, false, None, &[], Some(&store));
    assert_eq!(lines(&warm), lines(&full), "a warm saved run differs from a full run");
    assert_eq!(
        store.stores.load(Ordering::SeqCst),
        stores_after_cold,
        "a warm run with nothing moved must judge nothing new"
    );
    assert!(store.hits.load(Ordering::SeqCst) > 0, "and must have answered from the store");
}

#[test]
fn editing_one_file_judges_only_that_file_for_a_file_reach_lint() {
    let store = Memory::default();
    let (lint, runs) = Counting::boxed(Reach::File);
    let run = |root: &str, module: &str| {
        let fixture = crate_of(root, module);
        let before = runs.load(Ordering::SeqCst);
        let found = check_crate_saving(&fixture.ctx(), false, None, &lint, Some(&store));
        (found, runs.load(Ordering::SeqCst) - before)
    };

    assert_eq!(run(ROOT, MODULE).1, 2, "the control: a cold run judges both files");
    assert_eq!(run(ROOT, MODULE).1, 0, "nothing moved, so the lint must not run");

    let (found, edited) = run(ROOT, "pub fn m() { todo!() }\n// FIXME: new\n");
    assert_eq!(edited, 1, "one file moved, so exactly one file is judged");
    assert!(
        found.iter().any(|f| f.lint_name == "counting" && f.path.as_deref() == Some("src/m.rs")),
        "the edit's new finding must appear: {found:?}"
    );
}

#[test]
fn a_crate_reach_lint_reruns_when_any_file_of_the_crate_moves() {
    let store = Memory::default();
    let (lint, runs) = Counting::boxed(Reach::Crate);
    let run = |module: &str| {
        let fixture = crate_of(ROOT, module);
        let before = runs.load(Ordering::SeqCst);
        let _ = check_crate_saving(&fixture.ctx(), false, None, &lint, Some(&store));
        runs.load(Ordering::SeqCst) - before
    };
    assert_eq!(run(MODULE), 1, "cold: the crate is judged once");
    assert_eq!(run(MODULE), 0, "warm: nothing moved");
    assert_eq!(run("pub fn m() {}\n"), 1, "a module file moved, so the crate is judged again");
}

#[test]
fn a_lint_that_declares_nothing_runs_every_time() {
    let store = Memory::default();
    let (lint, runs) = Counting::boxed(Reach::Repository);
    let fixture = crate_of(ROOT, MODULE);
    for _ in 0 .. 3 {
        let _ = check_crate_saving(&fixture.ctx(), false, None, &lint, Some(&store));
    }
    assert_eq!(runs.load(Ordering::SeqCst), 6, "two files, three runs, nothing saved");
}

#[test]
fn a_configured_severity_is_applied_to_a_replayed_finding_and_does_not_invalidate_it() {
    let store = Memory::default();
    let fixture = crate_of(ROOT, MODULE);
    let ctx = fixture.ctx();
    let _ = check_crate_saving(&ctx, false, None, &[], Some(&store));
    let stores = store.stores.load(Ordering::SeqCst);

    let mut cfg = LintConfig::empty();
    cfg.base.insert("no-todo".to_string(), Severity::ADVISORY);
    let replayed = check_crate_saving(&ctx, false, Some(&cfg), &[], Some(&store));
    let fresh = check_crate_with_extra(&ctx, false, Some(&cfg), &[]);
    assert_eq!(lines(&replayed), lines(&fresh));
    assert!(
        replayed.iter().filter(|f| f.lint_name == "no-todo").all(|f| f.severity == Severity::ADVISORY),
        "the configured severity must reach a replayed finding"
    );
    assert_eq!(
        store.stores.load(Ordering::SeqCst),
        stores,
        "a severity change judges nothing again"
    );
}

#[test]
fn a_lint_parameter_is_part_of_the_key() {
    let store = Memory::default();
    let fixture = crate_of(ROOT, MODULE);
    let ctx = fixture.ctx();
    let _ = check_crate_saving(&ctx, false, None, &[], Some(&store));
    let stores = store.stores.load(Ordering::SeqCst);

    let mut cfg = LintConfig::empty();
    cfg.params.insert(
        "file-size".to_string(),
        HashMap::from([("max_lines".to_string(), "3".to_string())]),
    );
    let _ = check_crate_saving(&ctx, false, Some(&cfg), &[], Some(&store));
    assert!(
        store.stores.load(Ordering::SeqCst) > stores,
        "a configured parameter must judge the lint again"
    );
}

fn with_repo_context<R>(verdicts: Option<&dyn VerdictCache>, f: impl FnOnce(&crate::RepoContext) -> R) -> R {
    let crates = std::collections::BTreeSet::new();
    let registry = Default::default();
    let ctx = crate::RepoContext {
        mock_dir: std::path::Path::new("/mock"),
        repo_root: std::path::Path::new("/"),
        all_crates: &crates,
        src_dirs: &[],
        invocation: None,
        canon_paths: &[],
        open_panels: &[],
        registry: &registry,
        verdicts,
    };
    f(&ctx)
}

#[test]
fn a_repo_lint_asks_the_store_per_item_and_a_changed_part_asks_again() {
    let store = Memory::default();
    let computed = AtomicUsize::new(0);
    let judge = |ctx: &crate::RepoContext, item: &[u8]| {
        ctx.remembered("per-item", &[item], || {
            computed.fetch_add(1, Ordering::SeqCst);
            vec![LintError::error("kaski".to_string(), 0, "per-item", "stale".to_string())]
        })
    };
    with_repo_context(Some(&store), |ctx| {
        let first = judge(ctx, b"shader-a");
        assert_eq!(computed.load(Ordering::SeqCst), 1, "the control: a cold item is computed");
        assert_eq!(lines(&judge(ctx, b"shader-a")), lines(&first), "a replay gives the findings that were saved");
        assert_eq!(computed.load(Ordering::SeqCst), 1, "and does not compute again");
        judge(ctx, b"shader-b");
        assert_eq!(computed.load(Ordering::SeqCst), 2, "a different item is its own verdict");
    });
}

#[test]
fn a_repo_lint_without_a_store_computes_every_time() {
    let computed = AtomicUsize::new(0);
    with_repo_context(None, |ctx| {
        for _ in 0 .. 2 {
            ctx.remembered("per-item", &[b"x"], || {
                computed.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            });
        }
    });
    assert_eq!(computed.load(Ordering::SeqCst), 2);
}

#[test]
fn two_repo_lints_with_the_same_bytes_do_not_share_an_answer() {
    let store = Memory::default();
    with_repo_context(Some(&store), |ctx| {
        let a = ctx.remembered("lint-a", &[b"same"], || {
            vec![LintError::error("p".to_string(), 0, "lint-a", "a".to_string())]
        });
        let b = ctx.remembered("lint-b", &[b"same"], Vec::new);
        assert_eq!(a.len(), 1);
        assert!(b.is_empty(), "lint-b must not receive lint-a's findings");
    });
}
