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
        summary: "--namespace <ns> --path <dir under the mock directory> [--include <pattern>]... \
                  [--remote <name>]: declares a `[[data_ref]]`, makes its ref from the files the branch \
                  tracks there, and takes those files out of the branch's index, leaving them on disk as the \
                  instance",
        plan:    plan_directory_to_ref,
    },
    Migration {
        name:    "round-filenames",
        summary: "renames design-round files that carry a `YYYY-MM-DD_` prefix to the compact timestamp form",
        plan:    plan_round_filenames,
    },
    Migration {
        name:    "references-to-letkajenkka",
        summary: "none: rewrites every reference of the spelling before the letkajenkka standard (uppercase and file \
                  extensions in a name, `::#heading`, spaces beside a separator) in the `*.md.tmpl` files and the \
                  registry data (on a registry ref too) to the new spelling, and folds the preset shorthands of `extends` \
                  in `mockspace.toml`, where that reaches the same file and line, and names each reference it leaves",
        plan:    plan_references,
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
    after:     String,
}

#[derive(Debug)]
enum Step {
    /// Make the ref of `decl` from the files its directory holds and push it.
    Seed(DataRefDecl, Vec<String>),
    /// Record the directory of `decl` as the instance of the ref an earlier run made from it.
    Adopt(DataRefDecl),
    /// Append `text` to the config file.
    AppendConfig(String),
    /// Write these registry files (path under the registry root, text), kept on a ref, through the
    /// registry's own writer, one write for each ref they are held by.
    WriteRegistry(Vec<(String, String)>),
    /// Take these paths (from the repository root) out of the index, leaving the files.
    Untrack(Vec<String>),
    /// Keep git from writing over the instances of every ref the config declares once the steps
    /// before have run: the sparse checkout of this worktree and the hook that copies an edit. It
    /// carries the declarations the config holds by then, which the run checks before its first
    /// step, so a refusal of this one comes before anything is written.
    KeepGitOff(Vec<DataRefDecl>),
    /// Rename a file, both paths from the repository root.
    Rename {
        from: String,
        to:   String,
    },
    /// Write a file whole, its path from the repository root.
    Write {
        path: String,
        text: String,
    },
}

impl Plan {
    /// What is left to the caller once the run is through.
    #[cfg(test)]
    pub(crate) fn after(&self) -> &str {
        &self.after
    }
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
    if let Err(why) = preflight(cfg, &plan) {
        eprintln!("migrate: {why}");
        eprintln!("migrate: nothing was changed");
        return ExitCode::FAILURE;
    }
    for step in &plan.steps {
        if let Err(why) = carry_out(cfg, step) {
            eprintln!("migrate: stopped, and the steps before this one stand: {why}");
            eprintln!("migrate: the same call finishes it once what stopped it is out of the way");
            return ExitCode::FAILURE;
        }
    }
    println!("done: {}", plan.after);
    ExitCode::SUCCESS
}

/// Every refusal a step of `plan` would make part way, made before the first step: a copy that
/// cannot be kept in the cache, and whatever stops git being kept off the instances the config
/// declares once the run is through, a hook or a monitor of the person's own among them.
fn preflight(cfg: &Config, plan: &Plan) -> Result<(), String> {
    mockspace_store::kept::ready().map_err(|e| format!("no copy of a file can be kept in the cache before it is written: {e}"))?;
    for step in &plan.steps {
        if let Step::KeepGitOff(after) = step {
            let mut then = cfg.clone();
            then.data_refs = after.clone();
            crate::data_refs::keep_git_off_preflight(&then).map_err(|why| format!("keeping git from writing over the instance: {why}"))?;
        }
    }
    Ok(())
}

fn carry_out(cfg: &Config, step: &Step) -> Result<(), String> {
    match step {
        Step::Seed(decl, tracked) => {
            mark_seeded(cfg, decl)?;
            let store = decl.open(&cfg.repo_root)?;
            let inst = decl.instance(&store, &cfg.mock_dir)?;
            let message = format!("data: {}", decl.namespace());
            // the ref is made from what the branch tracks; a file that only sits in the directory is
            // left there, and reads as added against the ref
            let lead = tracked_lead(cfg, inst.dir());
            let keep: std::collections::BTreeSet<String> = tracked
                .iter()
                .filter_map(|p| p.strip_prefix(lead.as_str()).map(|r| r.trim_start_matches('/').to_owned()))
                .collect();
            inst.seed_tracked(&keep, &message, &mockspace_store::Refuse).map(|_| ()).map_err(|e| e.to_string())
        },
        // the seed of a run that was stopped inside it, finished here
        Step::Adopt(decl) => {
            mark_seeded(cfg, decl)?;
            let store = decl.open(&cfg.repo_root)?;
            let inst = decl.instance(&store, &cfg.mock_dir)?;
            inst.adopt().map(|_| ()).map_err(|e| e.to_string())
        },
        Step::AppendConfig(text) => {
            let held = std::fs::read_to_string(&cfg.config_path)
                .map_err(|e| format!("reading {}: {e}", cfg.config_path.display()))?;
            keep(&cfg.config_path)?;
            let lead =if held.ends_with("\n\n") || held.is_empty() {
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
            // by standard input, so a tree of any size is one call; the store's runner feeds the
            // list while it reads what git says, so a refusal ends the call with git's words
            let input: Vec<u8> = paths.iter().flat_map(|p| p.bytes().chain([0])).collect();
            let out = mockspace_store::Git::new(cfg.repo_root.clone())
                .run(&["update-index", "--force-remove", "-z", "--stdin"], &[], Some(&input))
                .map_err(|e| format!("running git update-index: {e}"))?;
            if out.ok() { Ok(()) } else { Err(format!("git update-index: {}", out.stderr)) }
        },
        // the declared instance is now what git would write over on a checkout of an older commit;
        // the config is read again, since a step before may have appended the declaration
        Step::KeepGitOff(_) => Config::try_from_dir(&cfg.mock_dir)
            .and_then(|again| crate::data_refs::keep_git_off_the_instances(&again))
            .map_err(|why| format!("keeping git from writing over the instance: {why}")),
        Step::Rename {
            from,
            to,
        } => {
            keep(&cfg.repo_root.join(from))?;
            keep(&cfg.repo_root.join(to))?;
            std::fs::rename(cfg.repo_root.join(from), cfg.repo_root.join(to))
                .map_err(|e| format!("renaming {from} to {to}: {e}"))
        },
        Step::Write {
            path,
            text,
        } => {
            // the plan refused it; a link made since would still take the write out of the tree
            if let Some(why) = crate::migrate_refs::symlinks_on(&cfg.repo_root, [path.as_str()].into_iter()).pop() {
                return Err(why);
            }
            keep(&cfg.repo_root.join(path))?;
            std::fs::write(cfg.repo_root.join(path), text).map_err(|e| format!("writing {path}: {e}"))
        },
        Step::WriteRegistry(files) => {
            mockspace_registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
            let stores =
                mockspace_registry::RegistryStores::from_decls(&cfg.mock_dir, &cfg.repo_root, &cfg.registry_stores)
                    .map_err(|e| e.to_string())?;
            let writer = mockspace_registry::RegistryWriter::new(cfg, &stores);
            let mut groups: std::collections::BTreeMap<String, Vec<(String, Option<Vec<u8>>)>> = Default::default();
            for (rel, text) in files {
                let dir = cfg
                    .registry_stores
                    .iter()
                    .filter_map(|d| d.dir.as_deref())
                    .map(|d| d.trim_matches('/'))
                    .find(|d| rel == d || rel.starts_with(&format!("{d}/")))
                    .ok_or_else(|| format!("{rel} is held by no registry ref"))?;
                groups.entry(dir.to_string()).or_default().push((rel.clone(), Some(text.clone().into_bytes())));
            }
            for (dir, changes) in groups {
                writer
                    .write(&changes, "chore: rewrite references to the letkajenkka spelling")
                    .map_err(|e| format!("the registry ref of `{dir}`: {e}"))?;
            }
            Ok(())
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

#[path = "migrate_directory_to_ref.rs"]
mod directory_to_ref;
use directory_to_ref::{plan_directory_to_ref, tracked_lead};

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
        after: "commit the renames on this branch".to_string(),
    })
}

fn plan_references(cfg: &Config, args: &[&str]) -> Result<Plan, Vec<String>> {
    flags(args, &[])?;
    let crate::migrate_refs::Planned { lines, writes, on_refs, instance_written } = crate::migrate_refs::plan(cfg)?;
    let mut steps: Vec<Step> = writes.into_iter().map(|(path, text)| Step::Write { path, text }).collect();
    if !on_refs.is_empty() {
        steps.push(Step::WriteRegistry(on_refs));
    }
    // A file of a data ref's instance is no part of any branch, so committing the branch leaves it.
    let after = if instance_written {
        "commit the rewritten files on this branch, and `cargo mock data-refs commit` for the ones a data ref holds"
    } else {
        "commit the rewritten files on this branch"
    };
    Ok(Plan {
        lines,
        steps,
        after: after.to_string(),
    })
}

#[path = "migrate_seeded.rs"]
mod seeded;
use seeded::{mark_seeded, seeded_here};

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "migrate_kept_tests.rs"]
mod kept_tests;

/// Keeps a copy of what stands at `path` in the person's cache before a step writes over, renames
/// or removes it (`mockspace_store::kept`); a copy that cannot be made stops the step.
fn keep(path: &std::path::Path) -> Result<(), String> {
    mockspace_store::kept::keep(path)
        .map(|_| ())
        .map_err(|e| format!("keeping a copy of {} before the step changes it: {e}", path.display()))
}
