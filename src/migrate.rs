//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock migrate <migration> [--dry-run] ...`: one named change of a repository's shape.
//!
//! Every call names the migration it makes and gives it everything it needs as flags. Nothing is
//! read off the tree to decide what to do: a migration checks that the tree is the one it was
//! written for, and refuses with the reason when it is not. A plan is made first, in full, and it
//! lists every path the migration would move, untrack or delete; `--dry-run` prints the plan and
//! stops, and a run without it carries out exactly that plan.
//!
//! A migration is a [`Migration`]: a name, a line of what it is for, and the function that plans
//! it. The list is [`MIGRATIONS`], and a new one is one more row of it.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::config::Config;
use crate::data_refs::DataRefDecl;

/// One named migration.
pub struct Migration {
    pub name:    &'static str,
    /// What it does and the flags it takes, for the listing a wrong call prints.
    pub summary: &'static str,
    plan:        fn(&Config, &[&str]) -> Result<Plan, Vec<String>>,
}

/// Every migration there is. A call names one of these or moves nothing.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name:    "directory-to-ref",
        summary: "--namespace <ns> --path <dir under the mock directory> [--include <pattern>]... [--form \
                  branch|ref] [--remote <name>]: declares a `[[data_ref]]`, makes its ref from the files the branch \
                  tracks there, and takes those files out of the branch's index, leaving them on disk as the \
                  instance",
        plan:    plan_directory_to_ref,
    },
    Migration {
        name:    "round-filenames",
        summary: "renames design-round files that carry a `YYYY-MM-DD_` prefix to the compact timestamp form",
        plan:    plan_round_filenames,
    },
];

/// The names of [`MIGRATIONS`].
pub fn names() -> Vec<&'static str> {
    MIGRATIONS.iter().map(|m| m.name).collect()
}

/// What a migration will do, decided before anything is done.
#[derive(Debug)]
pub struct Plan {
    /// One line for each thing the run changes, every path named. This is what a dry run prints.
    pub lines: Vec<String>,
    steps:     Vec<Step>,
    /// What is left to the caller once the run is through.
    after:     &'static str,
}

#[derive(Debug)]
enum Step {
    /// Make the ref of `decl` from the files its directory holds and push it.
    Seed(DataRefDecl, Vec<String>),
    /// Record the directory of `decl` as the instance of the ref an earlier run made from it.
    Adopt(DataRefDecl),
    /// Append `text` to the config file.
    AppendConfig(String),
    /// Take these paths (from the repository root) out of the index, leaving the files.
    Untrack(Vec<String>),
    /// Rename a file, both paths from the repository root.
    Rename {
        from: String,
        to:   String,
    },
}

/// The plan of the migration `args` name, or every reason there is none.
pub fn plan(cfg: &Config, args: &[&str]) -> Result<Plan, Vec<String>> {
    let rest: Vec<&str> = args.iter().copied().filter(|a| *a != "--dry-run").collect();
    let listing = || {
        MIGRATIONS
            .iter()
            .map(|m| format!("  {}  {}", m.name, m.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let Some((name, flags)) = rest.split_first() else {
        return Err(vec![format!("migrate: name the migration to make. The migrations:\n{}", listing())]);
    };
    match MIGRATIONS.iter().find(|m| m.name == *name) {
        Some(m) => (m.plan)(cfg, flags),
        None => Err(vec![format!("migrate: `{name}` is no migration. The migrations:\n{}", listing())]),
    }
}

/// Plans the migration, prints the plan, and unless `--dry-run` is given carries it out.
pub fn run(cfg: &Config, args: &[&str]) -> ExitCode {
    let dry = args.contains(&"--dry-run");
    let plan = match plan(cfg, args) {
        Ok(plan) => plan,
        Err(why) => {
            why.iter().for_each(|w| eprintln!("{w}"));
            return ExitCode::FAILURE;
        },
    };
    plan.lines.iter().for_each(|l| println!("{l}"));
    if dry {
        println!("dry run: nothing was changed");
        return ExitCode::SUCCESS;
    }
    for step in &plan.steps {
        if let Err(why) = carry_out(cfg, step) {
            eprintln!("migrate: stopped, and the steps before this one stand: {why}");
            eprintln!("migrate: the same call finishes it once what stopped it is out of the way");
            return ExitCode::FAILURE;
        }
    }
    // the declared instance is now what git would write over on a checkout of an older commit
    if plan.steps.iter().any(|s| matches!(s, Step::Untrack(_))) {
        let declared = Config::from_dir(&cfg.mock_dir);
        if let Err(why) = crate::data_refs::keep_git_off_the_instances(&declared) {
            eprintln!("migrate: the migration is made, and the sparse checkout that keeps git from writing over the instance is not set: {why}");
            return ExitCode::FAILURE;
        }
    }
    println!("done: {}", plan.after);
    ExitCode::SUCCESS
}

fn carry_out(cfg: &Config, step: &Step) -> Result<(), String> {
    match step {
        Step::Seed(decl, tracked) => {
            let store = decl.open(&cfg.repo_root)?;
            let inst = decl.instance(&store, &cfg.mock_dir)?;
            let message = format!("data: {}", decl.namespace());
            // the ref is made from what the branch tracks, so a tracked file a `.gitignore` also
            // matches goes in, and build output that only sits in the directory does not
            let lead = tracked_lead(cfg, inst.dir());
            let keep = tracked
                .iter()
                .filter_map(|p| p.strip_prefix(lead.as_str()).map(|r| r.trim_start_matches('/').to_owned()))
                .collect();
            inst.seed_keeping(&keep, &message, &mockspace_store::Refuse).map(|_| ()).map_err(|e| e.to_string())
        },
        Step::Adopt(decl) => {
            let store = decl.open(&cfg.repo_root)?;
            let inst = decl.instance(&store, &cfg.mock_dir)?;
            inst.adopt().map(|_| ()).map_err(|e| e.to_string())
        },
        Step::AppendConfig(text) => {
            let held = std::fs::read_to_string(&cfg.config_path)
                .map_err(|e| format!("reading {}: {e}", cfg.config_path.display()))?;
            let lead = if held.ends_with("\n\n") || held.is_empty() {
                ""
            } else if held.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            std::fs::write(&cfg.config_path, format!("{held}{lead}{text}"))
                .map_err(|e| format!("writing {}: {e}", cfg.config_path.display()))
        },
        Step::Untrack(paths) => {
            // by standard input, so a tree of any size is one call
            let mut child = Command::new("git")
                .arg("-C")
                .arg(&cfg.repo_root)
                .args(["update-index", "--force-remove", "-z", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("running git update-index: {e}"))?;
            {
                use std::io::Write;
                let mut input = child.stdin.take().ok_or("git update-index took no input")?;
                for p in paths {
                    input.write_all(p.as_bytes()).and_then(|()| input.write_all(b"\0")).map_err(|e| e.to_string())?;
                }
            }
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(format!("git update-index: {}", String::from_utf8_lossy(&out.stderr).trim()))
            }
        },
        Step::Rename {
            from,
            to,
        } => {
            std::fs::rename(cfg.repo_root.join(from), cfg.repo_root.join(to))
                .map_err(|e| format!("renaming {from} to {to}: {e}"))
        },
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("running git {}: {e}", args.first().unwrap_or(&"")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The flags of a call as (name, value) pairs. Every word must be a flag `known` names, followed
/// by its value; anything else is named in the refusal, all of them at once.
fn flags<'a>(args: &[&'a str], known: &[&str]) -> Result<Vec<(&'a str, &'a str)>, Vec<String>> {
    let (mut out, mut why) = (Vec::new(), Vec::new());
    let mut at = 0;
    while at < args.len() {
        let word = args[at];
        if !known.contains(&word) {
            why.push(format!("migrate: `{word}` is not a flag of this migration, which takes {}", known.join(", ")));
            at += 1;
            continue;
        }
        match args.get(at + 1).filter(|v| !v.starts_with("--")) {
            Some(value) => {
                out.push((word, *value));
                at += 2;
            },
            None => {
                why.push(format!("migrate: `{word}` takes a value"));
                at += 1;
            },
        }
    }
    if why.is_empty() { Ok(out) } else { Err(why) }
}

fn plan_directory_to_ref(cfg: &Config, args: &[&str]) -> Result<Plan, Vec<String>> {
    let given = flags(args, &["--namespace", "--path", "--include", "--form", "--remote"])?;
    let one = |name: &str| given.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| (*v).to_string());
    let include: Vec<String> =
        given.iter().filter(|(n, _)| *n == "--include").map(|(_, v)| (*v).to_string()).collect();
    let mut why = Vec::new();
    for required in ["--namespace", "--path"] {
        if one(required).is_none() {
            why.push(format!("migrate: `{required}` is not given, and it is never guessed"));
        }
    }
    if !why.is_empty() {
        return Err(why);
    }
    let decl = DataRefDecl {
        namespace: one("--namespace"),
        path:      one("--path"),
        instance:  None,
        remote:    one("--remote"),
        form:      one("--form"),
        include:   if include.is_empty() { None } else { Some(include) },
    };
    let ns = decl.namespace().to_string();
    let path = decl.path.clone().unwrap_or_default();

    // A run that stopped part way is finished by the same call. What it left is read off what
    // each step leaves behind: the seed leaves this clone an instance of the ref, the config
    // step leaves the table, and the last step leaves nothing tracked.
    let declared = cfg.data_refs.iter().any(|d| *d == decl);
    if !declared && cfg.data_refs.iter().any(|d| d.namespace() == ns || d.path.as_deref() == Some(path.as_str())) {
        return Err(vec![format!(
            "migrate: mockspace.toml already declares another `[[data_ref]]` for the namespace `{ns}` or the path \
             `{path}`"
        )]);
    }
    if !declared {
        let mut all = cfg.data_refs.clone();
        all.push(decl.clone());
        if let Err(mut bad) = crate::data_refs::validate_against(&all, &cfg.data_ref_taken) {
            bad.insert(0, "migrate: the declaration this would write is one the engine refuses:".to_string());
            return Err(bad);
        }
    }

    let tracked = decl.tracked(&cfg.repo_root, &cfg.mock_dir).map_err(|e| vec![format!("migrate: {e}")])?;
    if tracked.is_empty() {
        return Err(vec![if declared {
            format!("migrate: mockspace.toml already declares this ref and the branch tracks none of its files, so this migration was made here before")
        } else {
            format!(
                "migrate: the branch tracks no file under `{path}` that the ref would hold, so there is nothing to \
                 move and this is not a tree the migration was written for"
            )
        }]);
    }
    // A held file that differs from what the branch has committed would reach the ref in a form
    // no commit of the branch ever held, and an untracked one would reach it unseen.
    let mut pathspec = vec!["status", "--porcelain", "-z", "--no-renames", "--untracked-files=all", "--"];
    let dir = decl.instance_dir(&cfg.mock_dir);
    let dir_text = dir.to_string_lossy().into_owned();
    pathspec.push(&dir_text);
    let status = git(&cfg.repo_root, &pathspec).map_err(|e| vec![format!("migrate: {e}")])?;
    let held = match decl.include.as_deref() {
        None => None,
        Some(p) => Some(mockspace_store::Held::new(p).map_err(|e| vec![format!("migrate: {e}")])?),
    };
    let lead = tracked_lead(cfg, &dir);
    let changed: Vec<String> = status
        .split('\0')
        .filter(|e| e.len() > 3)
        .map(|e| e[3 ..].to_string())
        .filter(|p| {
            match (&held, p.strip_prefix(lead.as_str())) {
                (None, _) => true,
                (Some(h), Some(rel)) => h.holds(rel.trim_start_matches('/')),
                (Some(_), None) => false,
            }
        })
        .collect();
    if !changed.is_empty() {
        return Err(vec![format!(
            "migrate: {} file(s) the ref would hold differ from the branch or are not tracked ({}); commit or remove \
             them first, so the ref starts from what the branch holds",
            changed.len(),
            changed.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        )]);
    }
    let store = decl.open(&cfg.repo_root).map_err(|e| vec![format!("migrate: {e}")])?;
    let inst = decl.instance(&store, &cfg.mock_dir).map_err(|e| vec![format!("migrate: {e}")])?;
    let recorded = inst.base().map_err(|e| vec![format!("migrate: {e}")])?.is_some();
    // made here by a run that was stopped inside the seed: the ref has the files, no record says so
    let mut unrecorded = false;
    if recorded {
        // the files are tracked and clean, so what the ref holds is what the branch holds, unless
        // the directory was changed since
        if inst.is_dirty().map_err(|e| vec![format!("migrate: {e}")])? {
            return Err(vec![format!(
                "migrate: an earlier run made the ref of `{ns}` and the directory has changed since; \
                 `cargo mock data-refs commit {ns}` or put the files back, then run this again"
            )]);
        }
    } else {
        // No record here. Either the ref is not made yet, or a run made it and was stopped before
        // it recorded the directory, which shows as a ref holding exactly the files the branch
        // tracks. Anything else was made from other files, elsewhere.
        match mockspace_store::Store::refresh(&store) {
            Ok(mockspace_store::Freshness::Empty) if !declared => {},
            Ok(mockspace_store::Freshness::Empty) => {
                return Err(vec![format!(
                    "migrate: mockspace.toml declares the ref of `{ns}` and the remote has no commit of it; take the \
                     table out of mockspace.toml and run this again"
                )]);
            },
            Ok(_) => {
                if inst.is_the_tip().map_err(|e| vec![format!("migrate: {e}")])? {
                    unrecorded = true;
                } else {
                    return Err(vec![format!(
                        "migrate: the ref of `{ns}` already has commits and they do not hold what the branch tracks, \
                         so it was made elsewhere; `git rm --cached` the files the branch still tracks and `cargo \
                         mock data-refs instantiate {ns}` on a branch that declares it"
                    )]);
                }
            },
            Err(e) => {
                return Err(vec![format!("migrate: the remote could not be asked about the ref of `{ns}`: {e}")]);
            },
        }
    }

    let table = table_of(&decl);
    let (mut lines, mut steps) = (Vec::new(), Vec::new());
    if recorded {
        lines.push(format!("an earlier run of this migration made the ref of `{ns}`; finishing it"));
    } else if unrecorded {
        lines.push(format!(
            "an earlier run of this migration made the ref of `{ns}` and did not record the directory; finishing it"
        ));
        steps.push(Step::Adopt(decl));
    } else {
        lines.push(format!("make the ref of `{ns}` from {} file(s) and push it", tracked.len()));
        steps.push(Step::Seed(decl, tracked.clone()));
    }
    if !declared {
        lines.push(format!("append to {}:", cfg.config_path.display()));
        lines.extend(table.lines().map(|l| format!("  {l}")));
        steps.push(Step::AppendConfig(table));
    }
    lines.extend(tracked.iter().map(|p| format!("untrack {p}")));
    steps.push(Step::Untrack(tracked));
    Ok(Plan {
        lines,
        steps,
        after: "commit the config and the removals on this branch",
    })
}

/// The path of `dir` from the repository root, `/`-separated.
fn tracked_lead(cfg: &Config, dir: &Path) -> String {
    dir.strip_prefix(&cfg.repo_root)
        .map(|rel| rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// The `[[data_ref]]` table of `decl` as it is written to the config.
fn table_of(decl: &DataRefDecl) -> String {
    let quoted = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let mut out = String::from("[[data_ref]]\n");
    let mut put = |key: &str, value: &Option<String>| {
        if let Some(v) = value {
            out.push_str(&format!("{key} = {}\n", quoted(v)));
        }
    };
    put("namespace", &decl.namespace);
    put("path", &decl.path);
    put("remote", &decl.remote);
    put("form", &decl.form);
    if let Some(include) = &decl.include {
        let items: Vec<String> = include.iter().map(|p| quoted(p)).collect();
        out.push_str(&format!("include = [{}]\n", items.join(", ")));
    }
    out
}

fn plan_round_filenames(cfg: &Config, args: &[&str]) -> Result<Plan, Vec<String>> {
    flags(args, &[])?;
    let renames = crate::design_round::legacy_renames(cfg).map_err(|e| vec![format!("migrate: {e}")])?;
    if renames.is_empty() {
        return Err(vec![
            "migrate: no design-round file carries a `YYYY-MM-DD_` prefix, so this is not a tree the migration was \
             written for"
                .to_string(),
        ]);
    }
    let lines = renames.iter().map(|(from, to)| format!("rename {from} -> {to}")).collect();
    let steps = renames
        .into_iter()
        .map(|(from, to)| {
            Step::Rename {
                from,
                to,
            }
        })
        .collect();
    Ok(Plan {
        lines,
        steps,
        after: "commit the renames on this branch",
    })
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
