//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::LintPack;

use crate::config::Config;
use crate::{
    Level,
    LintMode,
    Severity,
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

pub(crate) mod cargo_gate;
mod dispatch;
pub(crate) mod escape_hatch;
pub(crate) mod help;
mod help_for;
mod message;
#[cfg(test)]
mod message_identity_tests;
pub(crate) use dispatch::*;
mod nuke;
pub(crate) use nuke::*;
mod panel;
mod subcmd;
mod test;
pub(crate) use subcmd::*;
mod resolve;
mod tool;
mod tool_writes;
pub(crate) use resolve::*;
mod check;
pub(crate) use check::*;
mod env;
pub(crate) use env::*;
#[cfg(test)]
mod clean_tests;

pub fn run() -> ExitCode {
    run_inner(&LintPack::default())
}

pub fn run_with_custom_lints(pack: LintPack) -> ExitCode {
    run_inner(&pack)
}
