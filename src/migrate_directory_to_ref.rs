//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The plan of `directory-to-ref`: which tracked files of the mock directory go to a new data ref,
//! the table that declares it, and the steps that make it, untrack them and keep git off them.

use std::path::Path;

use super::{Plan, Step, flags, git, seeded_here};
use crate::config::Config;
use crate::data_refs::DataRefDecl;

pub(super) fn plan_directory_to_ref(cfg: &Config, args: &[&str]) -> Result<Plan, Vec<String>> {
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
    // the last step left undone: everything is moved, the seed ran in this clone and left it its
    // instance, and git is not kept off the instance yet. A clone the seed did not run in was never
    // part of the run, whatever record of an instance it holds: a fresh clone of a repository
    // migrated elsewhere wants `data-refs instantiate`
    if declared
        && tracked.is_empty()
        && seeded_here(cfg, &decl)?
        && crate::data_refs::git_is_kept_off_the_instances(cfg).is_err()
    {
        return Ok(Plan {
            lines: vec![format!(
                "an earlier run of this migration moved `{ns}` and did not keep git from writing over its instance; \
                 finishing it"
            )],
            steps: vec![Step::KeepGitOff],
            after: "commit the config and the removals on this branch".to_string(),
        });
    }
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
                         so it was made elsewhere; `git rm --cached --sparse` the files the branch still tracks and `cargo \
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
    // a file a `.gitignore` matches stays where it is and out of the ref, as it stayed out of the branch
    let ignored = inst.ignored().map_err(|e| vec![format!("migrate: {e}")])?;
    let from_root = |p: &str| if lead.is_empty() { p.to_owned() } else { format!("{lead}/{p}") };
    lines.extend(ignored.iter().map(|p| format!("leave {}: ignored, not held", from_root(p))));
    if !declared {
        lines.push(format!("append to {}:", cfg.config_path.display()));
        lines.extend(table.lines().map(|l| format!("  {l}")));
        steps.push(Step::AppendConfig(table));
    }
    lines.extend(tracked.iter().map(|p| format!("untrack {p}")));
    steps.push(Step::Untrack(tracked));
    steps.push(Step::KeepGitOff);
    Ok(Plan {
        lines,
        steps,
        after: "commit the config and the removals on this branch".to_string(),
    })
}

/// The path of `dir` from the repository root, `/`-separated.
pub(super) fn tracked_lead(cfg: &Config, dir: &Path) -> String {
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
