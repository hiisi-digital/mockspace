use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::{CrossCrateLint, Lint};

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
    run_inner(&[], &[])
}

pub fn run_with_custom_lints(
    custom_lints: Vec<Box<dyn Lint>>,
    custom_cross_lints: Vec<Box<dyn CrossCrateLint>>,
) -> ExitCode {
    run_inner(&custom_lints, &custom_cross_lints)
}
