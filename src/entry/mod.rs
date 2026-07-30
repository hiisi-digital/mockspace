use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::LintPack;

use crate::config::Config;
use crate::{
    LintMode,
    bench,
    bootstrap,
    design_round,
    document,
    dylib_check,
    lint,
    parse,
    pdf,
    registry,
    render,
    render_agent,
    render_design,
    render_md,
};

mod cargo_gate;
mod help;
pub(crate) mod escape_hatch;
mod message;
mod dispatch;
pub(crate) use dispatch::*;
mod nuke;
pub(crate) use nuke::*;
mod subcmd;
pub(crate) use subcmd::*;
mod resolve;
pub(crate) use resolve::*;
mod check;
pub(crate) use check::*;
#[cfg(test)]
mod clean_tests;

pub fn run() -> ExitCode {
    run_inner(&LintPack::default())
}

pub fn run_with_custom_lints(pack: LintPack) -> ExitCode {
    run_inner(&pack)
}
