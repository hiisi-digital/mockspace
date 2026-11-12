//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Subcommands for managing design round lifecycle.
//!
//! `cargo mock lock`: lock the current phase's changelist.
//! `cargo mock deprecate`: deprecate the current unlocked changelist.
//! `cargo mock unlock`: destructive: nuke source, deprecate src CL, unlock doc CL.
//! `cargo mock close`: archive a completed round (CLOSED phase).
//! `cargo mock archive`: archive an abandoned round from any phase.
//! `cargo mock round-id`: print a fresh round id (see [`lint_rules::round_id`]).
//! `cargo mock rename-round`: give a round a new id, when two share one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules as lint_rules;
use mockspace_lint_rules::changelist_helpers::{self, ClKind, ClStatus, ParsedChangelist, Phase};

use crate::config::Config;

mod archive;
pub(crate) use archive::*;
mod files;
pub(crate) use files::*;
mod git;
pub(crate) use git::*;
mod rename;
pub use rename::{cmd_rename_round, cmd_round_id};
pub(crate) mod round_merge;

mod owed;
use owed::say_what_the_replacement_owes;
mod subjects;
#[cfg(test)]
mod tests;

/// A fresh reading rooted at `dir`, for one question asked of the round as it stands now. Fresh
/// each time, because a subcommand moves the files it asks about between one question and the
/// next.
pub(crate) fn reading_of(dir: &Path) -> lint_rules::Reading {
    lint_rules::Reading::new(dir)
}

/// Options parsed from CLI flags for design round subcommands.
pub struct SubcmdOpts {
    pub auto_commit: bool,
}

/// Refuse to lock a changelist that is a bare title.
///
/// Here as well as in the `changelist-seal` lint because `--auto-commit` commits
/// the lock without running a hook, so the lint would only ever see the lock
/// once it is history, where it no longer reads it.
fn refuse_an_empty_lock(dr: &Path, cl: &ParsedChangelist) -> Option<ExitCode> {
    if !lint_rules::says_nothing(&reading_of(&dr).view(), &dr.join(&cl.filename)) {
        return None;
    }
    eprintln!("error: {} holds nothing but a bare title", cl.filename);
    eprintln!("  a lock freezes what a changelist says; write it, then lock");
    Some(ExitCode::FAILURE)
}

/// Refuse to end the DOC window while a doc template it covers is not
/// committed. Past the lock `changelist-doc-gate` refuses every one of them,
/// so they would be stranded: reverted, or carried through a deprecation and a
/// second doc changelist, for want of one commit made first.
fn refuse_uncommitted_templates(cfg: &Config) -> Option<ExitCode> {
    let pending = match crate::data_refs::ref_files(cfg) {
        Ok(refs) => lint_rules::pending_doc_templates(&reading_of(&cfg.mock_dir).view(), &cfg.mock_dir, &cfg.src_dirs, &refs),
        Err(why) => {
            eprintln!("error: {why}");
            return Some(ExitCode::FAILURE);
        },
    };
    if pending.is_empty() {
        return None;
    }
    eprintln!("error: doc templates are not committed, and the lock ends the window they can be");
    for (file, how) in &pending {
        eprintln!("  {file} ({how})");
    }
    eprintln!("  commit them, then lock");
    Some(ExitCode::FAILURE)
}

pub fn cmd_lock(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&reading_of(&dr).view(), &dr);

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&reading_of(&dr).view(), &dr) {
                Some(cl) => cl,
                None => {
                    eprintln!(
                        "error: phase is DOC but the doc changelist it was read from is gone"
                    );
                    eprintln!(
                        "  {} is read twice and changed in between; re-run",
                        dr.display()
                    );
                    return ExitCode::FAILURE;
                },
            };
            if let Some(code) = refuse_an_empty_lock(&dr, &cl) {
                return code;
            }
            if let Some(code) = refuse_uncommitted_templates(cfg) {
                return code;
            }
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked doc changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: DOC → DRAFT");
                    eprintln!("  next: create a src changelist, then `cargo mock lock` again");
                    let msg = subjects::lock_doc(&r.new_name);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                },
            }
        },
        Phase::Src => {
            let cl = match changelist_helpers::find_active_src_cl(&reading_of(&dr).view(), &dr) {
                Some(cl) => cl,
                None => {
                    eprintln!(
                        "error: phase is IMPL but the src changelist it was read from is gone"
                    );
                    eprintln!(
                        "  {} is read twice and changed in between; re-run",
                        dr.display()
                    );
                    return ExitCode::FAILURE;
                },
            };
            if let Some(code) = refuse_an_empty_lock(&dr, &cl) {
                return code;
            }
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked src changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: IMPL → CLOSED");
                    eprintln!("  next: `cargo mock close` to archive the round");
                    let msg = subjects::lock_src(&r.new_name);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                },
            }
        },
        Phase::Topic => {
            eprintln!("error: no changelist to lock (TOPIC phase)");
            eprintln!("  create a doc changelist first");
            ExitCode::FAILURE
        },
        Phase::SrcPlan => {
            eprintln!("error: doc CL already locked, no src CL to lock (DRAFT phase)");
            eprintln!("  create a src changelist first");
            ExitCode::FAILURE
        },
        Phase::Done => {
            eprintln!("error: both changelists already locked (CLOSED phase)");
            eprintln!("  use `cargo mock close` to archive the round");
            ExitCode::FAILURE
        },
    }
}

// ---------------------------------------------------------------------------
// deprecate
// ---------------------------------------------------------------------------

pub fn cmd_deprecate(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&reading_of(&dr).view(), &dr);

    // A stub beside a locked changelist of its kind is what holds a round open whatever phase it
    // reads as, and it is the one thing `deprecate` is asked to retire there.
    let all_strays = changelist_helpers::stray_active_changelists(&reading_of(&dr).view(), &dr);
    // Only a stub is retired unread. One with text may be the round's live work beside the locks of
    // an earlier round, and renaming it would keep the dead locks and lose the work.
    let (strays, written): (Vec<_>, Vec<_>) = all_strays
        .into_iter()
        .partition(|cl| lint_rules::says_nothing(&reading_of(&dr).view(), &dr.join(&cl.filename)));
    if !written.is_empty() {
        for cl in &written {
            eprintln!(
                "error: {} holds text and sits beside a locked changelist of its kind",
                cl.filename
            );
        }
        eprintln!("  it may be this round's work and the lock an earlier round's, so it is not retired unread");
        eprintln!("  read both; `cargo mock unlock` reopens the round, or `git rm` the one that is not this round's");
        return ExitCode::FAILURE;
    }
    if !strays.is_empty() {
        let mut touched = Vec::new();
        for cl in &strays {
            match rename_cl(&dr, cl, ClStatus::Deprecated) {
                Ok(r) => {
                    eprintln!(
                        "deprecated {} changelist beside a locked one: {} → {}",
                        if cl.kind == ClKind::Doc { "doc" } else { "src" },
                        cl.filename,
                        r.new_name
                    );
                    touched.push(r.old_path);
                    touched.push(r.new_path);
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                },
            }
        }
        let names: Vec<&str> = strays.iter().map(|cl| cl.filename.as_str()).collect();
        let msg = subjects::deprecate_strays(names.len());
        commit_or_suggest(cfg, opts, &touched, &msg);
        return ExitCode::SUCCESS;
    }

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&reading_of(&dr).view(), &dr) {
                Some(cl) => cl,
                None => {
                    eprintln!(
                        "error: phase is DOC but the doc changelist it was read from is gone"
                    );
                    eprintln!(
                        "  {} is read twice and changed in between; re-run",
                        dr.display()
                    );
                    return ExitCode::FAILURE;
                },
            };
            match rename_cl(&dr, &cl, ClStatus::Deprecated) {
                Ok(r) => {
                    eprintln!(
                        "deprecated doc changelist: {} → {}",
                        cl.filename, r.new_name
                    );
                    eprintln!("  phase transition: DOC → TOPIC");
                    eprintln!("  next: create new topic files, then a new changelist");
                    say_what_the_replacement_owes(&r.new_path);
                    let msg = subjects::deprecate_doc(&cl.filename);
                    commit_or_suggest(cfg, opts, &[r.old_path, r.new_path], &msg);
                    ExitCode::SUCCESS
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                },
            }
        },
        Phase::Src => {
            let cl = match changelist_helpers::find_active_src_cl(&reading_of(&dr).view(), &dr) {
                Some(cl) => cl,
                None => {
                    eprintln!(
                        "error: phase is IMPL but the src changelist it was read from is gone"
                    );
                    eprintln!(
                        "  {} is read twice and changed in between; re-run",
                        dr.display()
                    );
                    return ExitCode::FAILURE;
                },
            };

            let mut touched = Vec::new();

            // Step 1: deprecate the src CL
            match rename_cl(&dr, &cl, ClStatus::Deprecated) {
                Ok(r) => {
                    eprintln!(
                        "deprecated src changelist: {} → {}",
                        cl.filename, r.new_name
                    );
                    say_what_the_replacement_owes(&r.new_path);
                    touched.push(r.old_path);
                    touched.push(r.new_path);
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                },
            }

            // Step 2: unlock the doc CL (DRAFT is a useless intermediate state)
            if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&reading_of(&dr).view(), &dr) {
                match rename_cl(&dr, &doc_cl, ClStatus::Active) {
                    Ok(r) => {
                        eprintln!(
                            "unlocked doc changelist: {} → {}",
                            doc_cl.filename, r.new_name
                        );
                        touched.push(r.old_path);
                        touched.push(r.new_path);
                    },
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    },
                }
            }

            eprintln!("  phase transition: IMPL → DOC");
            eprintln!("  next: update doc templates, then lock and create new src changelist");
            let msg = subjects::deprecate_src_and_unlock(&cl.filename);
            commit_or_suggest(cfg, opts, &touched, &msg);
            ExitCode::SUCCESS
        },
        Phase::Topic => {
            eprintln!("error: no changelist to deprecate (TOPIC phase)");
            ExitCode::FAILURE
        },
        Phase::SrcPlan => {
            eprintln!("error: doc CL is locked (DRAFT phase)");
            eprintln!("  use `cargo mock unlock` to unlock it first");
            ExitCode::FAILURE
        },
        Phase::Done => {
            eprintln!("error: both CLs locked (CLOSED phase)");
            eprintln!("  use `cargo mock unlock` to unlock the src CL first");
            ExitCode::FAILURE
        },
    }
}

// ---------------------------------------------------------------------------
// unlock
// ---------------------------------------------------------------------------

pub fn cmd_unlock(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&reading_of(&dr).view(), &dr);

    match phase {
        Phase::SrcPlan | Phase::Src | Phase::Done => {},
        _ => {
            eprintln!(
                "error: unlock requires a locked doc CL (current phase: {})",
                phase.label()
            );
            eprintln!("  unlock is only available in DRAFT, IMPL, or CLOSED phases");
            return ExitCode::FAILURE;
        },
    }

    eprintln!("WARNING: `unlock` is destructive.");
    eprintln!("  it will deprecate the src CL (if any) and unlock the doc CL.");
    eprintln!("  source changes made during IMPL phase are NOT automatically reverted.");
    eprintln!("  you must manually revert source changes if needed.");
    eprintln!();

    let mut touched = Vec::new();

    // Step 1: deprecate active or locked src CL if it exists.
    if let Some(src_cl) = changelist_helpers::find_active_src_cl(&reading_of(&dr).view(), &dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
                say_what_the_replacement_owes(&r.new_path);
                touched.push(r.old_path);
                touched.push(r.new_path);
            },
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    }
    if let Some(src_cl) = changelist_helpers::find_locked_src_cl(&reading_of(&dr).view(), &dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
                say_what_the_replacement_owes(&r.new_path);
                touched.push(r.old_path);
                touched.push(r.new_path);
            },
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    }

    // Step 2: unlock doc CL (rename *.lock.md → *.md).
    if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&reading_of(&dr).view(), &dr) {
        match rename_cl(&dr, &doc_cl, ClStatus::Active) {
            Ok(r) => {
                eprintln!("  unlocked doc CL: {} → {}", doc_cl.filename, r.new_name);
                touched.push(r.old_path);
                touched.push(r.new_path);
            },
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    }

    eprintln!();
    eprintln!("  phase transition: {} → DOC", phase.label());
    let msg = subjects::unlock_round();
    commit_or_suggest(cfg, opts, &touched, &msg);
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// close
// ---------------------------------------------------------------------------

pub fn cmd_close(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    cmd_close_taking(cfg, opts, &[])
}

/// `close`, taking the flat files named in `take` into the round as well.
pub fn cmd_close_taking(cfg: &Config, opts: &SubcmdOpts, take: &[String]) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&reading_of(&dr).view(), &dr);

    // A topic a close left flat, with no round open: `--take` files it under the newest closed
    // round, since the close that left it cannot be run again.
    if !take.is_empty() && !changelist_helpers::has_any_changelist(&reading_of(&dr).view(), &dr) {
        return adopt_into_last_round(cfg, opts, &dr, take);
    }

    if phase != Phase::Done {
        eprintln!(
            "error: can only close a round in CLOSED phase (current: {})",
            phase.label()
        );
        eprintln!("  both doc and src changelists must be locked");
        eprintln!("  for an abandoned round, use `cargo mock archive` instead");
        return ExitCode::FAILURE;
    }

    // Before anything moves: `--auto-commit` commits the close without a hook,
    // so the `changelist-seal` lint would first see this round as history.
    let findings = lint_rules::active_round_findings(&reading_of(&dr).view(), &dr);
    if !findings.is_empty() {
        eprintln!("error: the round is not sealed, so it is not closed");
        for finding in &findings {
            eprintln!("  {finding}");
        }
        return ExitCode::FAILURE;
    }

    let all_cls = changelist_helpers::find_changelists(&reading_of(&dr).view(), &dr);
    let round_name = determine_round_name(&all_cls);

    perform_archive(cfg, opts, &dr, &round_name, ArchiveKind::Closed, take)
}

/// Moves the flat files `take` names into the newest closed round of `dr`, the one with a `.meta`
/// and the greatest id. The files stay as written, so what a round's `.meta` records is unchanged.
fn adopt_into_last_round(
    cfg: &Config,
    opts: &SubcmdOpts,
    dr: &Path,
    take: &[String],
) -> ExitCode {
    let unknown: Vec<&String> = take
        .iter()
        .filter(|n| *n == "README.md" || n.contains('/') || !dr.join(n.as_str()).is_file())
        .collect();
    if !unknown.is_empty() {
        for name in unknown {
            eprintln!("error: --take {name}: no such flat file in design_rounds/");
        }
        return ExitCode::FAILURE;
    }
    let mut closed: Vec<String> = std::fs::read_dir(dr)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| lint_rules::round_id::is_round_dir(n))
        .filter(|n| dr.join(n).join(".meta").is_file())
        .collect();
    closed.sort();
    let Some(round) = closed.pop() else {
        eprintln!("error: no closed round in design_rounds/ to file `--take` under");
        return ExitCode::FAILURE;
    };
    // A topic stamped after the day the round closed is the next round's, not this one's.
    // The minute the closing commit was made, from the sha `.meta` recorded; the day alone only
    // where that commit cannot be read.
    let meta = std::fs::read_to_string(dr.join(&round).join(".meta")).unwrap_or_default();
    let closed_at: Option<String> = meta
        .lines()
        .find_map(|l| l.strip_prefix("closed:"))
        .and_then(|sha| git_commit_stamp(cfg, sha.trim()));
    let closed_on: String = closed_at.clone().unwrap_or_else(|| {
        meta.lines()
            .find_map(|l| l.strip_prefix("close_date:"))
            .map(|d| d.chars().filter(char::is_ascii_digit).collect())
            .unwrap_or_default()
    });
    let width = closed_on.len().min(12);
    let mut names: Vec<&String> = Vec::new();
    for name in take {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let mut refused = false;
    for name in &names {
        if dr.join(&round).join(name.as_str()).exists() {
            eprintln!("error: --take {name}: {round}/ already holds a file of that name, and a closed round's file is not overwritten");
            refused = true;
        }
        let stamped = lint_rules::round_id::split_prefix(name).map(|(id, _)| id[..id.len().min(width)].to_string());
        if let Some(at) = stamped
            && !closed_on.is_empty()
            && at > closed_on
        {
            let when = if closed_at.is_some() { "the minute" } else { "the day" };
            eprintln!("error: --take {name}: stamped after {when} {round} closed ({closed_on}), so it is the next round's and stays flat");
            refused = true;
        }
    }
    if refused {
        return ExitCode::FAILURE;
    }
    let mut touched = Vec::new();
    for name in &names {
        let from = dr.join(name.as_str());
        let to = dr.join(&round).join(name.as_str());
        if let Err(e) = std::fs::rename(&from, &to) {
            eprintln!("error: failed to move {name}: {e}");
            return ExitCode::FAILURE;
        }
        touched.push(from);
        touched.push(to);
    }
    eprintln!("moved {} files to {round}/", names.len());
    commit_or_suggest(cfg, opts, &touched, &format!("chore: file topics under round {round}"));
    ExitCode::SUCCESS
}

pub fn cmd_archive(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    if !dr.is_dir() {
        eprintln!("error: no design_rounds/ at {}", dr.display());
        eprintln!("  create it, or point --dir at the mock workspace that has one");
        return ExitCode::FAILURE;
    }

    // Named by the round's changelists where it has any, the way `close` names it, so the archive
    // carries the id of the round and not of whichever topic happens to be oldest. With none
    // (the TOPIC phase) the earliest file of any kind names it.
    let changelists = changelist_helpers::find_changelists(&reading_of(&dr).view(), &dr);
    let round_name = if !changelists.is_empty() {
        determine_round_name(&changelists)
    } else {
        match determine_round_name_from_dir(&dr) {
            Some(name) => name,
            None => {
                eprintln!(
                    "error: no round files to archive (design_rounds/ has no timestamp-prefixed \
                     files)"
                );
                return ExitCode::FAILURE;
            },
        }
    };

    perform_archive(
        cfg,
        opts,
        &dr,
        &format!("{round_name}-abandoned"),
        ArchiveKind::Abandoned,
        &[],
    )
}

/// The design-round files that carry a legacy `YYYY-MM-DD_` prefix, each with the name the compact
/// timestamp form gives it, both as paths from the repository root. A file whose new name is
/// taken is a refusal, since renaming over it would lose one of the two.
pub fn legacy_renames(cfg: &Config) -> Result<Vec<(String, String)>, String> {
    let dr = design_rounds_dir(cfg);
    let entries = fs::read_dir(&dr).map_err(|e| format!("no design_rounds/ at {}: {e}", dr.display()))?;
    let lead = dr
        .strip_prefix(&cfg.repo_root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("{} is outside the repository", dr.display()))?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != "README.md" && n.ends_with(".md"))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let Some(new_name) = legacy_to_new_filename(&name) else {
            continue;
        };
        if dr.join(&new_name).exists() {
            return Err(format!("{lead}/{name} would be renamed to {new_name}, which exists"));
        }
        let to = format!("{lead}/{new_name}");
        if let Some((other, _)) = out.iter().find(|(_, taken)| *taken == to) {
            return Err(format!("{other} and {lead}/{name} would both be renamed to {new_name}"));
        }
        out.push((format!("{lead}/{name}"), format!("{lead}/{new_name}")));
    }
    Ok(out)
}
