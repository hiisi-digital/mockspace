//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated agent rules and the write guard follow the declared source
//! directories, as the generated git hooks do, and not a literal `crates/`.
//!
//! The fixture is the grouped shape: a mock workspace with its crates under
//! `crates` and a console workspace with its own under `applets/console/crates`.

use std::fs;
use std::path::{Path, PathBuf};

use mockspace_lint_rules::glob_match;

use super::tests::write_guard_allows_in;
use super::*;

const GROUPED: &[&str] = &["crates", "applets/console/crates"];

fn grouped_cfg(repo: &Path) -> Config {
    let mock = repo.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = repo.to_path_buf();
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = GROUPED.iter().map(|d| mock.join(d)).collect();
    cfg
}

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// The globs in a generated rule's frontmatter, whichever list form it uses.
fn globs_of(rule: &str) -> Vec<String> {
    rule.lines()
        .take_while(|l| *l != "---" || rule.starts_with(*l))
        .flat_map(|l| {
            l.split('"')
                .skip(1)
                .step_by(2)
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn matches_any(globs: &[String], path: &str) -> bool {
    globs.iter().any(|g| glob_match(g, path))
}

#[test]
fn a_crate_under_a_second_source_directory_gets_a_rule_that_reaches_its_files() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let cfg = grouped_cfg(repo);
    write(&repo.join("mock/crates/sim/README.md.tmpl"), "# sim\n");
    write(
        &repo.join("mock/applets/console/crates/serve/README.md.tmpl"),
        "# serve\n",
    );
    let (claude, copilot) = (repo.join("claude"), repo.join("copilot"));
    fs::create_dir_all(&claude).unwrap();
    fs::create_dir_all(&copilot).unwrap();

    let n = generate_crate_readme_rules(
        &cfg,
        &claude,
        &copilot,
        &[],
        "",
        "",
        "",
        &crate::registry::Registry::default(),
    );
    assert_eq!(
        n, 4,
        "a Claude rule and a Copilot one for each of two crates"
    );

    let rule = fs::read_to_string(claude.join("crate-readme-serve.md")).unwrap();
    let globs = globs_of(&rule);
    assert!(
        matches_any(&globs, "mock/applets/console/crates/serve/src/lib.rs"),
        "the rule for a console crate matches none of its files: {globs:?}"
    );
    assert!(
        !matches_any(&globs, "mock/crates/serve/src/lib.rs"),
        "and it does not reach a crate of the same name in the other directory: {globs:?}"
    );
    // the control: the first directory's crate still gets its own glob
    let rule = fs::read_to_string(claude.join("crate-readme-sim.md")).unwrap();
    assert!(matches_any(&globs_of(&rule), "mock/crates/sim/src/lib.rs"));
}

fn forbidden_imports_cfg(repo: &Path) -> Config {
    let mut cfg = grouped_cfg(repo);
    let rule: std::collections::HashMap<String, String> = [
        ("rule.no-dyn.scope", "console-*"),
        ("rule.no-dyn.forbidden", "dyn"),
        ("rule.no-dyn.reason", "no dispatch"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    cfg.lint_overrides
        .params
        .insert("forbidden-imports".to_string(), rule);
    cfg
}

fn generate_forbidden(repo: &Path, cfg: &Config) -> (PathBuf, PathBuf) {
    let (rules, hooks) = (repo.join("rules"), repo.join("hooks"));
    for d in ["rules", "ci", "hooks", "chooks"] {
        fs::create_dir_all(repo.join(d)).unwrap();
    }
    generate_lint_derived_content(
        cfg,
        &rules,
        &repo.join("ci"),
        &hooks,
        &repo.join("chooks"),
        "",
        "",
        "",
    );
    (rules, hooks)
}

#[test]
fn a_forbidden_imports_rule_reaches_a_crate_under_each_source_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = forbidden_imports_cfg(tmp.path());
    let (rules, _) = generate_forbidden(tmp.path(), &cfg);
    let rule = fs::read_dir(&rules)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lint-forbidden-")
        })
        .expect("a forbidden-imports rule was written");
    let globs = globs_of(&fs::read_to_string(rule).unwrap());
    assert!(
        matches_any(
            &globs,
            "mock/applets/console/crates/console-core/src/lib.rs"
        ),
        "{globs:?}"
    );
    assert!(
        matches_any(&globs, "mock/crates/console-sim/src/lib.rs"),
        "{globs:?}"
    );
}

#[test]
fn the_forbidden_imports_hook_scopes_to_each_source_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = forbidden_imports_cfg(tmp.path());
    let (_, hooks) = generate_forbidden(tmp.path(), &cfg);
    let hook = fs::read_dir(&hooks)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lint-forbidden-")
        })
        .expect("a forbidden-imports hook was written");
    let text = fs::read_to_string(hook).unwrap();
    // the pattern the hook asks `grep -qE`, which is the behaviour and not its spelling
    let pattern = text
        .lines()
        .find_map(|l| l.split("grep -qE \"").nth(1))
        .and_then(|r| r.split('"').next())
        .expect("the hook scopes by a pattern")
        .to_string();
    let hits = |target: &str| {
        std::process::Command::new("grep")
            .args(["-qE", &pattern])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(target.as_bytes())?;
                c.wait()
            })
            .unwrap()
            .success()
    };
    assert!(
        hits("/r/mock/applets/console/crates/console-core/src/lib.rs"),
        "{pattern}"
    );
    assert!(hits("/r/mock/crates/console-sim/src/lib.rs"), "{pattern}");
    assert!(
        !hits("/r/mock/crates/other/src/lib.rs"),
        "out of scope: {pattern}"
    );
}

/// A source file under the second directory is phase-gated like one under the
/// first: the round is in DRAFT, which allows neither source nor frozen docs.
#[test]
#[cfg(unix)]
fn the_write_guard_gates_source_under_every_declared_directory() {
    let src = "applets/console/crates/console-core/src/lib.rs";
    let doc = "applets/console/crates/console-core/DESIGN.md.tmpl";
    assert!(
        !write_guard_allows_in(src, GROUPED),
        "source in DRAFT is refused"
    );
    assert!(
        !write_guard_allows_in(doc, GROUPED),
        "a frozen document is refused"
    );
    // the controls: with the directory undeclared the path is not a crate's, and
    // the first directory's source is still gated
    assert!(
        write_guard_allows_in(src, &["crates"]),
        "an undeclared directory is not gated"
    );
    assert!(!write_guard_allows_in("crates/foo/src/lib.rs", GROUPED));
}

#[test]
fn the_builtin_rules_name_and_reach_every_declared_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = grouped_cfg(tmp.path());
    let builtins = generate_builtin_templates(&cfg, &mockspace_lint_rules::LintPack::default());

    let chain = builtins
        .rules
        .iter()
        .find(|r| r.name == "canon-design-code-chain")
        .expect("the canon chain rule");
    assert!(
        matches_any(
            &chain.apply_to,
            "mock/applets/console/crates/c/DESIGN.md.tmpl"
        ),
        "{:?}",
        chain.apply_to
    );
    assert!(matches_any(
        &chain.apply_to,
        "mock/applets/console/crates/c/src/lib.rs"
    ));
    let readmes = builtins.rules.iter().find(|r| r.name == "readmes").unwrap();
    assert!(matches_any(
        &readmes.apply_to,
        "mock/applets/console/crates/c/README.md.tmpl"
    ));

    for r in &builtins.rules {
        assert!(
            !r.body.contains("{src_dirs}"),
            "`{}` kept the placeholder",
            r.name
        );
    }
    for s in &builtins.skills {
        assert!(
            !s.body.contains("{src_dirs}"),
            "`{}` kept the placeholder",
            s.dir_name
        );
    }
    let all: String = builtins.rules.iter().map(|r| r.body.as_str()).collect();
    assert!(
        all.contains("`mock/applets/console/crates/*/`"),
        "the gate prose names the second directory"
    );
}

/// A project whose crates are in `groups/a` and `groups/b` and in no directory
/// called `crates`: the fixture the literal cannot hide in, since `GROUPED`
/// contains one.
const RENAMED: &[&str] = &["groups/a", "groups/b"];

fn renamed_cfg(repo: &Path) -> Config {
    let mock = repo.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = repo.to_path_buf();
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = RENAMED.iter().map(|d| mock.join(d)).collect();
    cfg
}

/// Every piece of text a generated rule or skill carries.
fn generated_texts(b: &BuiltinTemplates) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for r in &b.rules {
        out.push((format!("rule {}", r.name), r.body.clone()));
        out.extend(
            r.apply_to
                .iter()
                .map(|g| (format!("rule {} glob", r.name), g.clone())),
        );
    }
    for s in &b.skills {
        out.push((format!("skill {}", s.dir_name), s.body.clone()));
        out.push((
            format!("skill {} description", s.dir_name),
            s.skill_description.clone(),
        ));
    }
    out.push(("preamble".to_string(), b.preamble.clone()));
    out.push(("postamble".to_string(), b.postamble.clone()));
    out
}

/// Each line of `texts` that names a directory called `crates`.
fn naming_crates_dir(texts: &[(String, String)]) -> Vec<String> {
    texts
        .iter()
        .flat_map(|(what, text)| {
            text.lines()
                .filter(|l| l.contains("crates/"))
                .map(move |l| format!("{what}: {}", l.trim()))
        })
        .collect()
}

#[test]
fn no_generated_rule_or_skill_names_a_directory_called_crates() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = renamed_cfg(tmp.path());
    let builtins = generate_builtin_templates(&cfg, &mockspace_lint_rules::LintPack::default());
    let texts = generated_texts(&builtins);
    // the control: the declared directories are named, so the sweep read prose
    // that is about them
    let all: String = texts.iter().map(|(_, t)| t.as_str()).collect();
    assert!(
        all.contains("`mock/groups/a/*/`"),
        "the declaration is not in the prose at all"
    );
    assert!(
        texts.len() > 40,
        "{} texts; the sweep is looking at too little",
        texts.len()
    );
    assert_eq!(naming_crates_dir(&texts), Vec::<String>::new());
}

/// The control for the sweep above: with the default declaration the same texts
/// are free to say `crates`, so what the sweep asserts is that the prose follows
/// the declaration and not that the word is banned.
#[test]
fn the_sweep_sees_a_literal_crates_directory_when_there_is_one() {
    let texts = vec![(
        "skill mockup-workflow".to_string(),
        "```\nmock/crates/<crate>/\n```\nedit `crates/<crate>/DESIGN.md.tmpl`\n".to_string(),
    )];
    assert_eq!(naming_crates_dir(&texts).len(), 2);
}
