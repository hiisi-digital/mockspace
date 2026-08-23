//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock panel {seat|consolidate|status}`: the CLI over [`crate::panel`].
//!
//! Thin on purpose. Every decision (what a seat number is, when a
//! consolidation is required, what "open" means) lives in `crate::panel`
//! and is tested there without touching a filesystem or a clock beyond what
//! the tests hand it explicitly. This module's whole job is: parse the
//! verb, load the named inventory, call the pure function, save it back,
//! and report what happened in words.

#![allow(unused_imports)]
use super::*;

use crate::panel::{self, MintRefusal, PanelInventory};

/// Unix seconds, for stamping a seat or a consolidation. The one place in
/// this CLI layer that reads the clock, so every call below it stays a pure
/// function taking `now_unix` explicitly.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn run(cfg: &Config, args: &[&str]) -> ExitCode {
    let Some((&verb, rest)) = args.split_first() else {
        eprintln!("mock panel: needs a verb");
        print_usage();
        return ExitCode::from(2);
    };
    match verb {
        "seat" => cmd_seat(cfg, rest),
        "consolidate" => cmd_consolidate(cfg, rest),
        "status" => cmd_status(cfg, rest),
        other => {
            eprintln!("mock panel: `{other}` is not a panel verb (seat, consolidate, status)");
            print_usage();
            ExitCode::from(2)
        },
    }
}

fn print_usage() {
    eprintln!("  usage:");
    eprintln!("    mock panel seat <slug> <persona> <topic...>");
    eprintln!("    mock panel consolidate <slug> <note...>");
    eprintln!("    mock panel status [slug]");
}

fn load_or_report(cfg: &Config, slug: &str) -> Result<(std::path::PathBuf, PanelInventory), ExitCode> {
    let path = panel::inventory_path(&cfg.mock_dir, slug);
    panel::load(&path, slug).map(|inv| (path, inv)).map_err(|e| {
        eprintln!("mock panel: {e}");
        ExitCode::FAILURE
    })
}

fn cmd_seat(cfg: &Config, args: &[&str]) -> ExitCode {
    let (Some(&slug), Some(&persona), topic_words) = (args.first(), args.get(1), args.get(2 ..))
    else {
        eprintln!("mock panel seat: usage: mock panel seat <slug> <persona> <topic...>");
        return ExitCode::from(2);
    };
    let Some(topic_words) = topic_words else {
        eprintln!("mock panel seat: needs a topic after the persona");
        return ExitCode::from(2);
    };
    if topic_words.is_empty() {
        eprintln!("mock panel seat: needs a topic after the persona");
        return ExitCode::from(2);
    }
    let topic = topic_words.join(" ");

    // Load, mint and save under one lock. Doing them separately is a
    // read-modify-write, and two dispatches racing it both mint the same
    // number while the second write erases the first.
    let path = panel::inventory_path(&cfg.mock_dir, slug);
    let cadence = cfg.panel_consolidate_every;
    match panel::with_locked(&path, slug, |inv| {
        panel::mint_seat(inv, persona, &topic, cadence, now_unix()).map_err(|r| r.to_string())
    }) {
        Ok(number) => {
            println!(
                "mock panel: seat {number}/{} minted on `{slug}` for {persona}: {topic}",
                panel::SEAT_CAP
            );
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!("mock panel seat: refused on `{slug}`: {why}");
            ExitCode::from(2)
        },
    }
}

fn cmd_consolidate(cfg: &Config, args: &[&str]) -> ExitCode {
    let (Some(&slug), note_words) = (args.first(), args.get(1 ..)) else {
        eprintln!("mock panel consolidate: usage: mock panel consolidate <slug> <note...>");
        return ExitCode::from(2);
    };
    let Some(note_words) = note_words else {
        eprintln!("mock panel consolidate: needs a note");
        return ExitCode::from(2);
    };
    if note_words.is_empty() {
        eprintln!("mock panel consolidate: needs a note");
        return ExitCode::from(2);
    }
    let note = note_words.join(" ");

    let path = panel::inventory_path(&cfg.mock_dir, slug);
    match panel::with_locked(&path, slug, |inv| {
        panel::consolidate(inv, &note, now_unix())
            .ok_or_else(|| "nothing new to consolidate".to_string())
    }) {
        Ok(through) => {
            println!("mock panel: `{slug}` consolidated through seat {through}");
            ExitCode::SUCCESS
        },
        Err(why) => {
            eprintln!("mock panel consolidate: {why} on `{slug}`");
            ExitCode::from(2)
        },
    }
}

fn cmd_status(cfg: &Config, args: &[&str]) -> ExitCode {
    let inventories: Vec<PanelInventory> = if let Some(&slug) = args.first() {
        match load_or_report(cfg, slug) {
            Ok((_, inv)) => vec![inv],
            Err(code) => return code,
        }
    } else {
        panel::load_all(&cfg.mock_dir)
    };

    if inventories.is_empty() {
        println!("mock panel: no panels declared under {}/panel/", cfg.mock_dir.display());
        return ExitCode::SUCCESS;
    }

    for inv in &inventories {
        let cadence = panel::effective_cadence(inv, cfg.panel_consolidate_every);
        let since = panel::seats_since_last_consolidation(inv);
        let state = if panel::is_open(inv) {
            "open"
        } else {
            "consolidated"
        };
        println!("mock panel: `{}`, {} seat(s), {state}", inv.slug, inv.seats.len());
        match panel::next_seat_number(inv) {
            Some(n) => println!("  next seat: {n} (cap {})", panel::SEAT_CAP),
            None => println!("  seat cap reached ({})", panel::SEAT_CAP),
        }
        if cadence > 0 {
            println!("  {since}/{cadence} seat(s) minted since the last consolidation");
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_in(dir: &std::path::Path) -> Config {
        let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
        c.mock_dir = dir.to_path_buf();
        c.repo_root = dir.to_path_buf();
        c
    }

    #[test]
    fn seating_writes_a_file_a_second_seat_reads_back() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());

        assert_eq!(cmd_seat(&cfg, &["kickoff", "leroy", "does", "it", "hold"]), ExitCode::SUCCESS);
        let path = crate::panel::inventory_path(&cfg.mock_dir, "kickoff");
        assert!(path.exists(), "seating must write the inventory file");
        let inv = crate::panel::load(&path, "kickoff").unwrap();
        assert_eq!(inv.seats.len(), 1);
        assert_eq!(inv.seats[0].persona, "leroy");
        assert_eq!(inv.seats[0].topic, "does it hold");

        assert_eq!(cmd_seat(&cfg, &["kickoff", "fallin", "lowering", "path"]), ExitCode::SUCCESS);
        let inv = crate::panel::load(&path, "kickoff").unwrap();
        assert_eq!(inv.seats.len(), 2, "the second seat must be appended, not overwrite");
        assert_eq!(inv.seats[1].number, 2);
    }

    #[test]
    fn seating_without_a_topic_is_refused_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());
        assert_eq!(cmd_seat(&cfg, &["slug", "persona"]), ExitCode::from(2));
        assert!(!crate::panel::inventory_path(&cfg.mock_dir, "slug").exists());
    }

    #[test]
    fn consolidating_with_no_seats_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());
        assert_eq!(cmd_consolidate(&cfg, &["slug", "nothing", "to", "say"]), ExitCode::from(2));
    }

    #[test]
    fn the_cadence_gate_reaches_the_cli_and_recovers_after_consolidating() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = cfg_in(tmp.path());
        cfg.panel_consolidate_every = 1;

        assert_eq!(cmd_seat(&cfg, &["s", "p", "one"]), ExitCode::SUCCESS);
        assert_eq!(
            cmd_seat(&cfg, &["s", "p", "two"]),
            ExitCode::from(2),
            "the cadence must refuse the second seat before a consolidation"
        );
        assert_eq!(cmd_consolidate(&cfg, &["s", "converged"]), ExitCode::SUCCESS);
        assert_eq!(
            cmd_seat(&cfg, &["s", "p", "two"]),
            ExitCode::SUCCESS,
            "minting must resume once consolidated"
        );
    }

    #[test]
    fn status_on_a_project_with_no_panels_says_so_rather_than_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());
        assert_eq!(cmd_status(&cfg, &[]), ExitCode::SUCCESS);
    }

    #[test]
    fn an_unknown_verb_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());
        assert_eq!(run(&cfg, &["frobnicate"]), ExitCode::from(2));
    }

    #[test]
    fn a_missing_verb_is_refused() {
        // `panel` is declared in `help::COMMANDS` with `verb` as a required
        // `ArgSpec`, the first builtin to declare one. Nothing shared
        // enforces that the way `missing_required` enforces a tool's
        // declared arguments (builtins are never audited by that
        // mechanism); this pins that the declaration is still true, because
        // `run` itself refuses with no verb at all, which is what the
        // shared mechanism would have done if it reached builtins.
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_in(tmp.path());
        assert_eq!(run(&cfg, &[]), ExitCode::from(2));
    }
}
