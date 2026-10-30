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
         `mock tools` and `mock lints` list everything here, with the severities that govern; below is only what this repository declares.\n\n\
         ```\n{tools}\n{set_here}```\n"
    )
}
