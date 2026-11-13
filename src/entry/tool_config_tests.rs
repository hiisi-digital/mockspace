//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A tool's `[tools.<name>]` table, from the config on disk to the tool's run: a key the tool reads
//! reaches it and it runs, and a key it does not read stops the run before the tool is entered.

use std::sync::{Arc, Mutex};

use mockspace_lint_rules::LintPack;
use mockspace_lint_rules::tool::{
    ArgSpec,
    ConfigKey,
    NotALint,
    Purpose,
    Tool,
    ToolContext,
    ToolParams,
    ToolReport,
};

use crate::config::Config;

/// What the probe was handed, whether it ran, and the code the run exited with, kept outside it so
/// the test reads it after the run.
#[derive(Default)]
struct Seen {
    subjects: Option<Vec<String>>,
    ran:      bool,
    exit:     Option<std::process::ExitCode>,
}

struct Probe(Arc<Mutex<Seen>>);

const KEYS: &[ConfigKey] = &[ConfigKey {
    name:        "subjects",
    description: "the subject ids the probe reads",
}];

impl Tool for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn description(&self) -> &'static str {
        "records what it was handed"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::TakesAQuestion)
    }

    fn args(&self) -> &[ArgSpec] {
        &[ArgSpec {
            name:        "question",
            required:    true,
            description: "what is asked",
        }]
    }

    fn config_keys(&self) -> &[ConfigKey] {
        KEYS
    }

    fn configure(&mut self, params: &ToolParams) {
        self.0.lock().unwrap().subjects = params.list("subjects").map(<[String]>::to_vec);
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        self.0.lock().unwrap().ran = true;
        ToolReport::reported(String::new(), 1)
    }
}

/// Loads a repository whose config is `toml`, configures a pack holding the probe from it through
/// the dispatcher's own `configure_pack`, and runs the probe, returning what it saw.
fn run_with(toml: &str) -> Seen {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("mock/crates")).unwrap();
    std::fs::write(
        repo.join("mockspace.toml"),
        format!("src_dirs = [\"crates\"]\n{toml}"),
    )
    .unwrap();
    let cfg = Config::from_dir(&repo.join("mock"));
    let seen = Arc::new(Mutex::new(Seen::default()));
    let mut pack = LintPack::default();
    pack.tools.push(Box::new(Probe(Arc::clone(&seen))));
    crate::entry::dispatch::configure_pack(&mut pack, &cfg);
    let exit = super::tool::run(&cfg, &pack, "probe", &["anything"], true);
    drop(pack);
    let mut seen = Arc::try_unwrap(seen)
        .ok()
        .expect("the pack is dropped")
        .into_inner()
        .unwrap();
    seen.exit = Some(exit);
    seen
}

#[test]
fn a_key_the_tool_reads_reaches_it_and_it_runs() {
    let seen = run_with("\n[tools.probe]\nsubjects = [\"workflow\", \"engine\"]\n");
    assert_eq!(
        seen.subjects,
        Some(vec!["workflow".to_owned(), "engine".to_owned()])
    );
    assert!(seen.ran);
    assert_eq!(seen.exit, Some(std::process::ExitCode::SUCCESS));
}

#[test]
fn a_key_the_tool_does_not_read_stops_the_run_and_never_reaches_it() {
    // `subject` for `subjects`: the misspelling a project makes, which before the refusal ran the
    // tool on its defaults and reported as though the value had applied.
    let seen = run_with("\n[tools.probe]\nsubject = [\"workflow\"]\n");
    assert_eq!(seen.subjects, None, "the undeclared key was handed over");
    assert!(!seen.ran, "the tool ran over a key it does not read");
    assert_eq!(seen.exit, Some(std::process::ExitCode::from(2)));
}

#[test]
fn with_no_table_the_tool_runs_on_its_defaults() {
    // THE CONTROL: the two above are a table's doing and not the probe's, so with none it is handed
    // nothing and runs.
    let seen = run_with("");
    assert_eq!(seen.subjects, None);
    assert!(seen.ran);
    assert_eq!(seen.exit, Some(std::process::ExitCode::SUCCESS));
}

#[test]
fn mock_help_names_the_keys_a_tool_reads() {
    let mut pack = LintPack::default();
    pack.tools.push(Box::new(Probe(Arc::default())));
    let listings = crate::tool_catalogue::enumerate(&pack);
    let probe = listings
        .iter()
        .find(|l| l.name == "probe")
        .expect("the probe is listed");
    let help = crate::tool_catalogue::render_one(probe);
    assert!(
        help.contains("[tools.probe] subjects  the subject ids the probe reads"),
        "{help}"
    );
    // THE CONTROL: a tool that reads no table has no key named in its help.
    let other = listings
        .iter()
        .find(|l| l.name != "probe")
        .expect("another tool is listed");
    assert!(
        !crate::tool_catalogue::render_one(other).contains("[tools."),
        "a tool reading no table names one in its help"
    );
}
