//! The driver: one fixture's mock dir in, every edge and every finding of both arms out.
//!
//! It reads the fixture the way the v1 engine reads a consumer, through `Config::from_dir` and
//! `parse::discover_crates_in`, and judges it through `mockspace_lint_rules::run`, the runner the
//! engine's `lint::run_lints` calls. What it carries of its own is listed against the shipped file:
//!
//! - the crate inputs are built as `src/lint/mod.rs` builds them in `run_lints` (lines 273 to 286:
//!   the crate's directory is the first source directory holding one of its name, and a crate with no
//!   `src/lib.rs` is skipped), and the workspace input as `workspace_input` there (lines 43 to 59);
//! - the repo input is that of `run_lints` (lines 298 to 308), with an empty registry, no canon
//!   paths, no open panels and no ref files, none of which this lint reads;
//! - the pack is configured as `configure_pack` in `src/entry/dispatch.rs` (lines 243 to 246) does
//!   for a loaded pack, from the config's `lint_overrides`;
//! - `only` names one lint per run, so no builtin lint runs over the fixture, and there is no
//!   verdict store, so every unit is judged rather than replayed.
//!
//! Arm B is handed the parameters configured for arm A's name, so both arms judge one rule set.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mockspace::config::Config;
use mockspace_lint_rules::src_layout::NO_REF_FILES;
use mockspace_lint_rules::{CrateInput, LintPack, Reading, RegistryView, RepoInput, RunInput, WorkspaceInput};
use sketch_edges_across_source_directories::{
    ByEngineDeps, ByManifest, NAME, NAME_BY_ENGINE, Pairs, Verdict, declared_names, judge, manifest_edges, packages,
};

fn main() -> ExitCode {
    let Some(mock) = std::env::args().nth(1) else {
        eprintln!("usage: driver <mock dir>");
        return ExitCode::from(2);
    };
    let mock = std::fs::canonicalize(&mock).unwrap_or_else(|e| panic!("{mock}: {e}"));
    let cfg = Config::from_dir(&mock);

    println!("== config, as Config::from_dir read it");
    println!("mock_dir     {}", rel(&cfg.repo_root, &cfg.mock_dir));
    let declared: Vec<String> = declared_names(&cfg.mock_dir, &cfg.src_dirs).into_iter().map(|(n, _)| n).collect();
    println!("src_dirs     {declared:?}");
    println!("crate_prefix {}", cfg.crate_prefix);
    let mut params: Vec<(String, String)> = cfg
        .lint_overrides
        .params
        .get(NAME)
        .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    params.sort();
    for (k, v) in &params {
        println!("param        [lints.{NAME}] {k} = {v}");
    }
    if params.is_empty() {
        println!("param        none under [lints.{NAME}]");
    }

    // Discovery, as the engine runs it.
    let crates = mockspace::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    println!("\n== crates the engine's discovery found (name, directory, deps it extracted)");
    if crates.is_empty() {
        println!("none");
    }

    let reading = Reading::new(&cfg.repo_root);
    let view = reading.view();
    let mut inputs: Vec<CrateInput> = Vec::new();
    for (name, info) in &crates {
        let Some(dir) = cfg.src_dirs.iter().map(|d| d.join(name)).find(|d| view.is_dir(d)) else { continue };
        if !view.is_file(dir.join("src/lib.rs")) {
            continue;
        }
        println!("{name:<12} {:<8} {:?}", rel(&cfg.mock_dir, dir.parent().unwrap()), info.deps);
        inputs.push(CrateInput { name: name.clone(), short_name: info.short_name.clone(), deps: info.deps.clone(), dir });
    }

    // Arm A's own reading of the same tree, printed so each verdict can be checked by hand.
    let pairs = cfg.lint_overrides.params.get(NAME).map(Pairs::from_params).unwrap_or_default();
    let pkgs = packages(&view, &cfg.mock_dir, &cfg.src_dirs);
    println!("\n== edges arm A read from the manifests, each with its verdict");
    let edges = manifest_edges(&view, &cfg.repo_root, &pkgs);
    if edges.is_empty() {
        println!("none ({} packages read)", pkgs.len());
    }
    for e in &edges {
        let verdict = match judge(&pairs, e) {
            Verdict::Within => "allowed, one directory".to_owned(),
            Verdict::Allowed(rule) => format!("allowed by rule `{rule}`"),
            Verdict::Refused => "REFUSED".to_owned(),
        };
        println!(
            "{} ({}) -> {} ({})  [{}]  {}:{}  {verdict}",
            e.from_crate,
            e.from_dir,
            e.to_crate,
            e.to_dir,
            e.section,
            e.manifest.as_deref().unwrap_or("-"),
            e.line
        );
    }

    // The pack, configured from the project's `[lints]` as the engine configures a loaded one.
    let mut pack = LintPack::default();
    pack.repo_lints.push(Box::new(ByManifest::default()));
    pack.workspace_lints.push(Box::new(ByEngineDeps::default()));
    pack.configure_from(&cfg.lint_overrides);
    let mut arm_b = ByEngineDeps::default();
    arm_b.pairs = pairs.clone();
    pack.workspace_lints[0] = Box::new(arm_b);

    let ws = WorkspaceInput {
        all_crates: crates.keys().cloned().collect::<BTreeSet<_>>(),
        workspace_root: cfg.mock_dir.clone(),
        proc_macro_crates: cfg.proc_macro_crates.clone(),
        lint_proc_macro_source: cfg.lint_proc_macro_source,
        crate_prefix: cfg.crate_prefix.clone(),
        primitive_introductions: Default::default(),
    };
    let registry = RegistryView::default();
    let repo = RepoInput {
        mock_dir: &cfg.mock_dir,
        repo_root: &cfg.repo_root,
        all_crates: &ws.all_crates,
        src_dirs: &cfg.src_dirs,
        invocation: None,
        canon_paths: &[],
        open_panels: &[],
        ref_files: &NO_REF_FILES,
        registry: &registry,
    };

    let mut total = 0usize;
    for lint in [NAME, NAME_BY_ENGINE] {
        let input = RunInput {
            reading: &reading,
            cache: None,
            overrides: Some(&cfg.lint_overrides),
            doc_only: false,
            workspace: &ws,
            crates: &inputs,
            scope: None,
            repo: Some(repo),
            only: Some(lint),
        };
        let found = mockspace_lint_rules::run(&input, Some(&pack));
        println!("\n== {lint}: {} finding(s)", found.len());
        for f in &found {
            println!("{}:{}  {}", f.path.as_deref().unwrap_or(&f.crate_name), f.line, f.message);
        }
        total += found.len();
    }
    println!("\n== {}", if total == 0 { "GREEN" } else { "RED" });
    if total == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).map(PathBuf::from).unwrap_or_else(|_| p.to_path_buf()).display().to_string()
}
