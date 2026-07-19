use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::{Lint, CrossCrateLint};

use crate::bench;
use crate::bootstrap;
use crate::document;
use crate::config::Config;
use crate::design_round;
use crate::registry;
use crate::pdf;
use crate::dylib_check;
use crate::lint;
use crate::parse;
use crate::render;
use crate::render_agent;
use crate::render_design;
use crate::render_md;
use crate::LintMode;


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

