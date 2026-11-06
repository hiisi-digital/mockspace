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
         `mock tools` and `mock lints` list everything here with the governing severities, including what arrives through `lint-crates`. The reach column is how much of the repository a lint reads (a lint declaring nothing is the widest); a run replays a saved verdict for a lint of narrower reach unless something it reads moved. A repository lint judging separable items replays each item (`remembered_batch`). The registry schema check replays while nothing taplo reads has moved. Below is only what this repository declares, as of the last `cargo mock`.\n\n\
         ```\n{tools}\n{set_here}```\n"
    )
}
