//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a unit is saved under: the lint's identity and configuration and the unit itself, so a
//! different configuration or a different lint of the same name never replays another's findings.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::{CrateLint, LintContext, PathFilter};

/// A crate lint reporting its parameter and the file it was shown, counting its judgings.
struct Param {
    name:   &'static str,
    value:  String,
    judged: Arc<AtomicUsize>,
}

impl Lint for Param {
    fn name(&self) -> &'static str {
        self.name
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        if let Some(v) = params.get("value") {
            self.value.clone_from(v);
        }
    }
}

impl CrateLint for Param {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        self.judged.fetch_add(1, Ordering::SeqCst);
        let at = ctx.source_path().display().to_string();
        vec![LintError::error(ctx.crate_name.to_string(), 1, self.name, format!("{}@{at}", self.value))]
    }
}

fn param(name: &'static str) -> (LintPack, Arc<AtomicUsize>) {
    let judged = Arc::new(AtomicUsize::new(0));
    let pack = LintPack {
        crate_lints: vec![Box::new(Param {
            name,
            value: "default".into(),
            judged: Arc::clone(&judged),
        })],
        ..LintPack::default()
    };
    (pack, judged)
}

fn workspace() -> Workspace {
    Workspace::new("keys", &[("alpha", &[("src/lib.rs", "pub mod m;\n"), ("src/m.rs", "pub fn f() { todo!() }\n")])])
}

fn of(name: &str, found: &[LintError]) -> Vec<String> {
    found
        .iter()
        .filter(|e| e.lint_name == name)
        .map(|e| format!("{}:{:?}", e.message, e.path))
        .collect()
}

fn with_param(value: &str) -> LintConfig {
    let mut cfg = LintConfig::empty();
    cfg.params
        .insert("param".into(), HashMap::from([("value".to_string(), value.to_string())]));
    cfg
}

#[test]
fn a_parameter_is_part_of_the_key() {
    let w = workspace();
    let cache = Mem::default();
    let (mut pack, judged) = param("param");
    let one = with_param("one");
    let two = with_param("two");

    // The engine configures a pack once while it owns it, as here.
    pack.configure_from(&one);
    let first = w.run(Some(&cache), Some(&one), Some(&pack));
    assert!(of("param", &first).iter().all(|m| m.starts_with("one@")), "{first:?}");
    let n = judged.load(Ordering::SeqCst);
    let _ = w.run(Some(&cache), Some(&one), Some(&pack));
    assert_eq!(judged.load(Ordering::SeqCst), n, "the same parameter replays");

    pack.configure_from(&two);
    let second = w.run(Some(&cache), Some(&two), Some(&pack));
    assert!(of("param", &second).iter().all(|m| m.starts_with("two@")), "{second:?}");
    assert_eq!(judged.load(Ordering::SeqCst), 2 * n);
}

#[test]
fn a_configured_severity_is_applied_to_replayed_findings() {
    let w = workspace();
    let cache = Mem::default();
    let mut warn = LintConfig::empty();
    warn.base.insert("no-todo".into(), Severity::ADVISORY);
    let level = |found: &[LintError]| {
        found
            .iter()
            .filter(|e| e.lint_name == "no-todo")
            .map(|e| e.severity)
            .collect::<Vec<_>>()
    };
    let judged = level(&w.run(Some(&cache), Some(&warn), None));
    assert_eq!(judged, [Severity::ADVISORY]);
    let reading = Reading::new(&w.dir.0);
    let replayed = level(&w.run_with(&reading, Some(&cache), Some(&warn), None));
    assert_eq!(replayed, judged);
    assert_eq!(reading.counts().parses, 0, "the control: the second run was a replay");
}

#[test]
fn a_pack_lint_with_a_built_in_name_saves_apart_from_it() {
    let w = workspace();
    let cache = Mem::default();
    let (pack, judged) = param("no-todo");
    let full = shown(&w.run(None, None, Some(&pack)));
    assert_eq!(of("no-todo", &w.run(None, None, Some(&pack))).len(), 3, "the built-in finds one, the pack lint two");
    let n = judged.load(Ordering::SeqCst);
    let per_run = n / 2;

    let cold = shown(&w.run(Some(&cache), None, Some(&pack)));
    let warm = shown(&w.run(Some(&cache), None, Some(&pack)));
    assert_eq!(cold, full);
    assert_eq!(warm, full, "each replays its own findings and not the other's");
    assert_eq!(judged.load(Ordering::SeqCst), n + per_run, "the pack lint was judged cold and replayed warm");
}

#[test]
fn a_path_filter_changing_reruns_the_lint() {
    let w = workspace();
    let cache = Mem::default();
    let mut only_root = LintConfig::empty();
    only_root.paths.insert("no-todo".into(), PathFilter {
        include: vec!["src/lib.rs".into()],
        exclude: vec![],
    });
    let all = shown(&w.run(Some(&cache), None, None));
    assert!(all.contains("no-todo"), "the control: unfiltered, the module's todo is found");
    let filtered = w.run(Some(&cache), Some(&only_root), None);
    assert_eq!(shown(&filtered), shown(&w.run(None, Some(&only_root), None)));
    assert!(of("no-todo", &filtered).is_empty(), "the filtered run judged again: {filtered:?}");
}

/// A crate-scoped lint naming the one file it was shown, which a path filter picks.
struct First;

impl Lint for First {
    fn name(&self) -> &'static str {
        "first"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn per_file(&self) -> bool {
        false
    }
}

impl CrateLint for First {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let shown = format!("{}={}", ctx.source_path().display(), ctx.source().trim());
        vec![LintError::error(ctx.crate_name.to_string(), 1, "first", shown)]
    }
}

/// Under a path filter the file a crate-scoped lint is shown is the first the filter admits, so a
/// file sorting ahead of it reruns the lint, though the lint read nothing but that one file.
#[test]
fn a_file_sorting_first_under_a_filter_reruns_the_lint() {
    let w = Workspace::new("keys-first", &[("alpha", &[("src/lib.rs", "pub mod mz;\n"), ("src/mz.rs", "z\n")])]);
    let cache = Mem::default();
    let pack = LintPack {
        crate_lints: vec![Box::new(First)],
        ..LintPack::default()
    };
    let mut filtered = LintConfig::empty();
    filtered.paths.insert("first".into(), PathFilter {
        include: vec!["src/m*.rs".into()],
        exclude: vec![],
    });
    assert_eq!(of("first", &w.run(Some(&cache), Some(&filtered), Some(&pack))), ["src/mz.rs=z:None"]);
    w.write("alpha", "src/ma.rs", "a\n");
    assert_eq!(
        of("first", &w.run(Some(&cache), Some(&filtered), Some(&pack))),
        ["src/ma.rs=a:None"],
        "the new first file is the one shown"
    );
}

/// A file the filter admits that cannot be read as text is left out of the files a crate-scoped
/// lint is shown, so its turning readable changes which file is first and reruns the lint.
#[test]
fn a_file_turning_readable_under_a_filter_reruns_the_lint() {
    let w = Workspace::new("keys-readable", &[("alpha", &[("src/lib.rs", "pub mod mz;\n"), ("src/mz.rs", "z\n")])]);
    let cache = Mem::default();
    let pack = LintPack {
        crate_lints: vec![Box::new(First)],
        ..LintPack::default()
    };
    let mut filtered = LintConfig::empty();
    filtered.paths.insert("first".into(), PathFilter {
        include: vec!["src/m*.rs".into()],
        exclude: vec![],
    });
    std::fs::write(w.dir.0.join("crates/alpha/src/ma.rs"), [0xff, 0xfe, b'\n']).unwrap();
    assert_eq!(
        of("first", &w.run(Some(&cache), Some(&filtered), Some(&pack))),
        ["src/mz.rs=z:None"],
        "the control: a file that is not text is not shown"
    );
    w.write("alpha", "src/ma.rs", "a\n");
    assert_eq!(
        of("first", &w.run(Some(&cache), Some(&filtered), Some(&pack))),
        ["src/ma.rs=a:None"],
        "the file that turned readable is the one shown"
    );
}

/// A crate lint carrying the crate's absolute directory into its finding, or not, counting its
/// judgings.
struct Where {
    name:     &'static str,
    absolute: bool,
    judged:   Arc<AtomicUsize>,
}

impl Lint for Where {
    fn name(&self) -> &'static str {
        self.name
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn per_file(&self) -> bool {
        false
    }
}

impl CrateLint for Where {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        self.judged.fetch_add(1, Ordering::SeqCst);
        let shown = if self.absolute {
            ctx.crate_dir().display().to_string()
        } else {
            ctx.source().trim().to_string()
        };
        vec![LintError::error(ctx.crate_name.to_string(), 1, self.name, shown)]
    }
}

/// Two checkouts of the same files share the store. A unit that read only files replays in the
/// other checkout; a unit handed the absolute crate directory is judged again there, so its finding
/// names the checkout it is shown in.
#[test]
fn a_unit_handed_an_absolute_path_replays_only_under_its_own_root() {
    let files: &[(&str, &[(&str, &str)])] = &[("alpha", &[("src/lib.rs", "pub fn f() {}\n")])];
    let one = Workspace::new("root-one", files);
    let two = Workspace::new("root-two", files);
    let cache = Mem::default();
    let (abs, rel) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let pack = LintPack {
        crate_lints: vec![
            Box::new(Where {
                name:     "where-abs",
                absolute: true,
                judged:   Arc::clone(&abs),
            }),
            Box::new(Where {
                name:     "where-rel",
                absolute: false,
                judged:   Arc::clone(&rel),
            }),
        ],
        ..LintPack::default()
    };
    let at_one = one.run(Some(&cache), None, Some(&pack));
    let at_two = two.run(Some(&cache), None, Some(&pack));
    let dir = |w: &Workspace| w.dir.0.join("crates/alpha").display().to_string();
    assert_eq!(of("where-abs", &at_one), [format!("{}:None", dir(&one))]);
    assert_eq!(of("where-abs", &at_two), [format!("{}:None", dir(&two))], "named the checkout it ran in");
    assert_eq!(abs.load(Ordering::SeqCst), 2);
    assert_eq!(of("where-rel", &at_two), of("where-rel", &at_one));
    assert_eq!(rel.load(Ordering::SeqCst), 1, "the control: a unit that read only files replays in the other checkout");
}

/// A crate lint naming the directory a program it starts finds itself in, asked by a path relative
/// to the repository root.
struct Pwd;

impl Lint for Pwd {
    fn name(&self) -> &'static str {
        "pwd"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn per_file(&self) -> bool {
        false
    }
}

impl CrateLint for Pwd {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let out = ctx.view().run("pwd", &[], std::path::Path::new("crates"), &[]);
        let shown = out.text().unwrap_or_default().trim().to_string();
        vec![LintError::error(ctx.crate_name.to_string(), 1, "pwd", shown)]
    }
}

/// A program is started in an absolute directory however its directory was named, so what it says
/// can carry the checkout, and a unit that started one replays only under its own root.
#[test]
fn a_unit_that_started_a_program_replays_only_under_its_own_root() {
    let files: &[(&str, &[(&str, &str)])] = &[("alpha", &[("src/lib.rs", "pub fn f() {}\n")])];
    let one = Workspace::new("pwd-one", files);
    let two = Workspace::new("pwd-two", files);
    let cache = Mem::default();
    let pack = LintPack {
        crate_lints: vec![Box::new(Pwd)],
        ..LintPack::default()
    };
    let dir = |w: &Workspace| {
        let real = std::fs::canonicalize(w.dir.0.join("crates")).unwrap();
        [real.display().to_string(), w.dir.0.join("crates").display().to_string()]
    };
    let at_one = of("pwd", &one.run(Some(&cache), None, Some(&pack)));
    let at_two = of("pwd", &two.run(Some(&cache), None, Some(&pack)));
    assert!(dir(&one).iter().any(|d| at_one == [format!("{d}:None")]), "the control: {at_one:?}");
    assert!(dir(&two).iter().any(|d| at_two == [format!("{d}:None")]), "named the checkout it ran in: {at_two:?}");
}
