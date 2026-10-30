//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The body of the generated `catalogues` rule.
//!
//! The rule loads on the first file read in a repository, so what it carries is
//! paid for by every session there. It carries only what the repository decided:
//! the tools it declares and the lints it set itself. What is the same in every
//! repository, the builtin subcommands and every lint running at the pack's
//! default, is one command away and answered live by `mock tools` and
//! `mock lints`, which is where the rule points.

use mockspace_lint_rules::LintPack;

use crate::config::Config;

/// The rule body for `cfg` against the loaded `pack`.
pub(crate) fn body(cfg: &Config, pack: &LintPack) -> String {
    let tools = crate::tool_catalogue::render_project_table(&crate::tool_catalogue::enumerate(pack));
    let lints = crate::lint_catalogue::enumerate(pack, &cfg.lint_overrides);
    let absent = crate::lint_catalogue::configured_but_absent(pack, &cfg.lint_overrides);
    let set_here = crate::lint_catalogue::render_set_here(&lints, &absent);
    format!(
        "## What is available here, and what is checking you\n\n\
         `mock tools` lists every subcommand and project tool and `mock lints` every lint with the severity that governs it; `mock help <name>` says what one is held to. Run them before writing a check, and never conclude from a directory listing that a repository has no tools: tools and lints also arrive through `lint-crates` in `mockspace.toml`. Below is only what this repository declares, as of the last `cargo mock`; the commands are the truth.\n\n\
         ```\n{tools}\n{set_here}```\n"
    )
}
