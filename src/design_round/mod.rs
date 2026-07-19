//! Subcommands for managing design round lifecycle.
//!
//! `cargo mock lock`: lock the current phase's changelist.
//! `cargo mock deprecate`: deprecate the current unlocked changelist.
//! `cargo mock unlock`: destructive: nuke source, deprecate src CL, unlock doc CL.
//! `cargo mock close`: archive a completed round (CLOSED phase).
//! `cargo mock archive`: archive an abandoned round from any phase.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::changelist_helpers::{self, ClKind, ClStatus, ParsedChangelist, Phase};

use crate::config::Config;

mod archive;
pub(crate) use archive::*;
mod files;
pub(crate) use files::*;
mod git;
pub(crate) use git::*;

#[cfg(test)]
mod tests;

/// Options parsed from CLI flags for design round subcommands.
pub struct SubcmdOpts {
    pub auto_commit: bool,
}

pub fn cmd_lock(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active doc changelist found");
                    return ExitCode::FAILURE;
                },
            };
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked doc changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: DOC → DRAFT");
                    eprintln!("  next: create a src changelist, then `cargo mock lock` again");
                    let msg = format!("chore: lock doc changelist for {}", r.new_name);
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
            let cl = match changelist_helpers::find_active_src_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active src changelist found");
                    return ExitCode::FAILURE;
                },
            };
            match rename_cl(&dr, &cl, ClStatus::Locked) {
                Ok(r) => {
                    eprintln!("locked src changelist: {} → {}", cl.filename, r.new_name);
                    eprintln!("  phase transition: IMPL → CLOSED");
                    eprintln!("  next: `cargo mock close` to archive the round");
                    let msg = format!("chore: lock src changelist for {}", r.new_name);
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
    let phase = changelist_helpers::current_phase(&dr);

    match phase {
        Phase::Doc => {
            let cl = match changelist_helpers::find_active_doc_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active doc changelist found");
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
                    let msg = format!("chore: deprecate doc changelist {}", cl.filename);
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
            let cl = match changelist_helpers::find_active_src_cl(&dr) {
                Some(cl) => cl,
                None => {
                    eprintln!("error: no active src changelist found");
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
                    touched.push(r.old_path);
                    touched.push(r.new_path);
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                },
            }

            // Step 2: unlock the doc CL (DRAFT is a useless intermediate state)
            if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&dr) {
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
            let msg = format!(
                "chore: deprecate src changelist {} and unlock doc CL",
                cl.filename
            );
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
    let phase = changelist_helpers::current_phase(&dr);

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
    if let Some(src_cl) = changelist_helpers::find_active_src_cl(&dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
                touched.push(r.old_path);
                touched.push(r.new_path);
            },
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    }
    if let Some(src_cl) = changelist_helpers::find_locked_src_cl(&dr) {
        match rename_cl(&dr, &src_cl, ClStatus::Deprecated) {
            Ok(r) => {
                eprintln!("  deprecated src CL: {} → {}", src_cl.filename, r.new_name);
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
    if let Some(doc_cl) = changelist_helpers::find_locked_doc_cl(&dr) {
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
    let msg = "chore: unlock design round (deprecate src CL, unlock doc CL)";
    commit_or_suggest(cfg, opts, &touched, msg);
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// close
// ---------------------------------------------------------------------------

pub fn cmd_close(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    let phase = changelist_helpers::current_phase(&dr);

    if phase != Phase::Done {
        eprintln!(
            "error: can only close a round in CLOSED phase (current: {})",
            phase.label()
        );
        eprintln!("  both doc and src changelists must be locked");
        eprintln!("  for an abandoned round, use `cargo mock archive` instead");
        return ExitCode::FAILURE;
    }

    let all_cls = changelist_helpers::find_changelists(&dr);
    let round_name = determine_round_name(&all_cls);

    perform_archive(cfg, opts, &dr, &round_name, ArchiveKind::Closed)
}

pub fn cmd_archive(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);
    if !dr.is_dir() {
        eprintln!("error: design_rounds/ directory not found");
        return ExitCode::FAILURE;
    }

    let round_name = match determine_round_name_from_dir(&dr) {
        Some(name) => name,
        None => {
            eprintln!(
                "error: no round files to archive (design_rounds/ has no timestamp-prefixed files)"
            );
            return ExitCode::FAILURE;
        },
    };

    perform_archive(
        cfg,
        opts,
        &dr,
        &format!("{round_name}-abandoned"),
        ArchiveKind::Abandoned,
    )
}

pub fn cmd_migrate(cfg: &Config, opts: &SubcmdOpts) -> ExitCode {
    let dr = design_rounds_dir(cfg);

    if !dr.is_dir() {
        eprintln!("error: design_rounds/ directory not found");
        return ExitCode::FAILURE;
    }

    let entries: Vec<_> = fs::read_dir(&dr)
        .expect("can't read design_rounds")
        .flatten()
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();

    let mut touched = Vec::new();
    let mut renamed = 0u32;
    let mut skipped = 0u32;

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip README.md and non-.md files.
        if name == "README.md" || !name.ends_with(".md") {
            continue;
        }

        // Skip files already in new format (12-digit prefix).
        if !is_legacy_filename(&name) {
            skipped += 1;
            continue;
        }

        let new_name = match legacy_to_new_filename(&name) {
            Some(n) => n,
            None => {
                eprintln!("  skip (unrecognized): {name}");
                skipped += 1;
                continue;
            },
        };

        let old_path = entry.path();
        let new_path = dr.join(&new_name);

        if new_path.exists() {
            eprintln!("  skip (target exists): {name} → {new_name}");
            skipped += 1;
            continue;
        }

        fs::rename(&old_path, &new_path)
            .unwrap_or_else(|e| panic!("failed to rename {name} → {new_name}: {e}"));

        eprintln!("  {name} → {new_name}");
        touched.push(old_path);
        touched.push(new_path);
        renamed += 1;
    }

    if renamed == 0 {
        eprintln!("nothing to migrate ({skipped} files already in new format or skipped)");
        return ExitCode::SUCCESS;
    }

    eprintln!("migrated {renamed} file(s), skipped {skipped}");
    let msg = format!("chore: migrate {renamed} design round file(s) to new naming convention");
    commit_or_suggest(cfg, opts, &touched, &msg);
    ExitCode::SUCCESS
}
