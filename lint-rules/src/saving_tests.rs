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
    check_crate_named, check_crate_with_extra,
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

/// Findings as lines, in the order the run returned them: a replay is compared with a fresh run in
/// order too, since the order is part of what a project reads and of what makes a replay equal.
fn lines(found: &[LintError]) -> Vec<String> {
    verdict::encode(found).lines().skip(1).map(String::from).collect()
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
            vec![LintError::error("alpha".to_string(), 0, "per-item", "stale".to_string())]
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

/// A pack lint that shares a built-in's name and reads the same file but finds something else.
struct Shadow;

impl Lint for Shadow {
    fn name(&self) -> &'static str {
        "no-todo"
    }

    fn reach(&self) -> Reach {
        Reach::File
    }
}

impl CrateLint for Shadow {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        ctx.source
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("XXX"))
            .map(|(i, _)| LintError::error(ctx.crate_name.to_string(), i + 1, "no-todo", "shadow".into()))
            .collect()
    }
}

#[test]
fn a_pack_lint_with_a_built_ins_name_does_not_replay_the_built_ins_verdict() {
    let fixture = LintFixture::new("pub fn a() { todo!() }\n// XXX\n");
    let ctx = fixture.ctx();
    let pack: Vec<Box<dyn CrateLint>> = vec![Box::new(Shadow)];
    let full = check_crate_with_extra(&ctx, false, None, &pack);
    assert!(
        full.iter().any(|f| f.message == "shadow") && full.len() >= 2,
        "the control: both lints find something: {full:?}"
    );
    let store = Memory::default();
    for run in ["cold", "warm"] {
        let saved = check_crate_saving(&ctx, false, None, &pack, Some(&store));
        assert_eq!(lines(&saved), lines(&full), "{run}: the shadowing lint was answered by the other");
    }
}

/// A workspace lint that reports the number of crates it was shown, once, and counts its runs.
struct Census {
    runs: Arc<AtomicUsize>,
}

impl Lint for Census {
    fn name(&self) -> &'static str {
        "census"
    }

    fn reach(&self) -> Reach {
        Reach::Workspace
    }
}

impl crate::WorkspaceLint for Census {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let fixmes: usize = crates.iter().map(|(_, c)| c.source.matches("FIXME").count()).sum();
        vec![LintError::error("workspace".to_string(), 1, "census", format!("{} crates, {fixmes} fixme", crates.len()))]
    }
}

#[test]
fn a_workspace_lint_replays_until_a_bound_crate_moves() {
    use crate::check_workspace_saving;
    let runs = Arc::new(AtomicUsize::new(0));
    let pack: Vec<Box<dyn crate::WorkspaceLint>> = vec![Box::new(Census { runs: Arc::clone(&runs) })];
    let store = Memory::default();
    let one = crate_of(ROOT, MODULE);
    let two = LintFixture::new("pub fn b() {}\n");
    let census = |first: &LintFixture, second: &LintFixture| {
        let (a, b) = (first.ctx(), second.ctx());
        let crates: [(&str, &LintContext); 2] = [("alpha", &a), ("beta", &b)];
        let saved = check_workspace_saving(&crates, false, None, &pack, Some(&store));
        let full = crate::check_workspace_with_extra(&crates, false, None, &pack);
        (saved, full)
    };

    let (saved, full) = census(&one, &two);
    assert_eq!(lines(&saved), lines(&full), "cold");
    assert!(saved.iter().any(|f| f.message == "2 crates, 1 fixme"), "the control: the lint reported: {saved:?}");
    // the two calls above ran the lint once saved and once not
    let before = runs.load(Ordering::SeqCst);
    let (saved, full) = census(&one, &two);
    assert_eq!(lines(&saved), lines(&full), "warm");
    assert_eq!(runs.load(Ordering::SeqCst) - before, 1, "warm: only the unsaved full run executes the lint");

    let edited = LintFixture::new("pub fn b() {}\n// FIXME: new\n");
    let before = runs.load(Ordering::SeqCst);
    let (saved, full) = census(&one, &edited);
    assert_eq!(lines(&saved), lines(&full), "after a crate moved");
    assert!(saved.iter().any(|f| f.message == "2 crates, 2 fixme"), "{saved:?}");
    assert_eq!(runs.load(Ordering::SeqCst) - before, 2, "a moved crate runs the lint again");
}

fn stores_after_changing(change: impl FnOnce(LintFixture) -> LintFixture) -> (usize, usize) {
    let store = Memory::default();
    let before = crate_of(ROOT, MODULE);
    let _ = check_crate_saving(&before.ctx(), false, None, &[], Some(&store));
    let saved = store.stores.load(Ordering::SeqCst);
    let after = change(crate_of(ROOT, MODULE));
    let _ = check_crate_saving(&after.ctx(), false, None, &[], Some(&store));
    (saved, store.stores.load(Ordering::SeqCst))
}

#[test]
fn a_changed_crate_scalar_judges_the_file_lints_again() {
    let (control_before, control_after) = stores_after_changing(|f| f);
    assert_eq!(control_before, control_after, "the control: the same crate judges nothing new");

    let (before, after) = stores_after_changing(|f| f.with_crate_prefix("other"));
    assert!(after > before, "a changed crate prefix must judge again");
    let (before, after) = stores_after_changing(|f| f.with_crate_name("renamed", "renamed"));
    assert!(after > before, "a changed crate name must judge again");
    let (before, after) = stores_after_changing(|f| f.with_crate_name("test-crate", "other-short"));
    assert!(after > before, "a changed short name alone must judge again");
    let (before, after) = stores_after_changing(|f| f.with_proc_macro_crates(&["test-crate"]));
    assert!(after > before, "a crate becoming a proc-macro crate must judge again");
    let (before, after) = stores_after_changing(|f| f.with_lint_proc_macro_source(true));
    assert!(after > before, "a changed proc-macro source answer must judge again");
    let (before, after) = stores_after_changing(|f| {
        f.with_primitive_introductions(std::collections::BTreeMap::from([(
            "test-crate".to_string(),
            vec!["u8".to_string()],
        )]))
    });
    assert!(after > before, "a changed introduced primitive must judge again");
}

#[test]
fn a_changed_path_filter_judges_the_lint_again() {
    let store = Memory::default();
    let fixture = crate_of(ROOT, MODULE);
    let ctx = fixture.ctx();
    let _ = check_crate_saving(&ctx, false, None, &[], Some(&store));
    let saved = store.stores.load(Ordering::SeqCst);
    let _ = check_crate_saving(&ctx, false, None, &[], Some(&store));
    assert_eq!(store.stores.load(Ordering::SeqCst), saved, "the control: nothing moved");

    let mut cfg = LintConfig::empty();
    cfg.paths.insert(
        "no-todo".to_string(),
        crate::path_filter::PathFilter { include: vec!["src/m.rs".to_string()], exclude: Vec::new() },
    );
    let _ = check_crate_saving(&ctx, false, Some(&cfg), &[], Some(&store));
    assert!(store.stores.load(Ordering::SeqCst) > saved, "a path filter must judge the lint again");
}

/// A repo lint whose verdict depends on a parameter it holds itself.
struct Tunable {
    runs: Arc<AtomicUsize>,
}

impl Lint for Tunable {
    fn name(&self) -> &'static str {
        "tunable"
    }
}

impl crate::RepoLint for Tunable {
    fn check_repo(&self, ctx: &crate::RepoContext) -> Vec<LintError> {
        ctx.remembered("tunable", &[b"item"], || {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        })
    }
}

#[test]
fn a_repo_lints_configuration_is_part_of_what_it_remembers() {
    let store = Memory::default();
    let runs = Arc::new(AtomicUsize::new(0));
    let params = |max: &str| {
        let mut cfg = LintConfig::empty();
        cfg.params.insert(
            "tunable".to_string(),
            HashMap::from([("max".to_string(), max.to_string())]),
        );
        cfg
    };
    let run = |cfg: &LintConfig| {
        with_repo_context(Some(&store), |ctx| {
            let extra: Vec<Box<dyn crate::RepoLint>> = vec![Box::new(Tunable { runs: Arc::clone(&runs) })];
            crate::check_repo_with_extra(ctx, false, Some(cfg), &extra)
        })
    };
    run(&params("3"));
    run(&params("3"));
    assert_eq!(runs.load(Ordering::SeqCst), 1, "the control: the same configuration replays");
    run(&params("5"));
    assert_eq!(runs.load(Ordering::SeqCst), 2, "a changed parameter must not replay the old verdict");
}

#[test]
fn a_named_run_judges_afresh_every_time() {
    let fixture = crate_of(ROOT, MODULE);
    let ctx = fixture.ctx();
    let (pack, runs) = Counting::boxed(Reach::File);

    let first = check_crate_named(&ctx, "counting", None, &pack);
    assert!(!first.is_empty(), "the control: the lint has a finding to return");
    let after_first = runs.load(Ordering::SeqCst);
    assert!(after_first > 0, "the named lint must have run");

    // The same call again: a replayed verdict would leave the counter where it was.
    let second = check_crate_named(&ctx, "counting", None, &pack);
    assert_eq!(lines(&second), lines(&first));
    assert_eq!(
        runs.load(Ordering::SeqCst),
        after_first * 2,
        "a named run must judge afresh each time, there being no store for it to read"
    );

    assert!(check_crate_named(&ctx, "no-such-lint", None, &pack).is_empty());
}
