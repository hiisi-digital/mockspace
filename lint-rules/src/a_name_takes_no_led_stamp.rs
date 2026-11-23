//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital

//! No file or directory of the repository is named the way a reference cites a stamped name.
//!
//! The standard lets no name open on a digit, so the host's fold leads such a name with a letter:
//! `202611011713_renderer-roadmap.md` is cited as `t202611011713_renderer_roadmap`
//! (`mockspace_refnames::fold`). A file of its own called `t202611011713_renderer-roadmap.md` beside
//! it would answer the same citation, and every citation of the first would turn ambiguous the day
//! the second appeared. So such a name is refused where it is made, whatever stands beside it.
//! What counts as that spelling is `mockspace_refnames`'s, asked through `Folded::takes_a_led_stamp`:
//! the lead and a stamp of twelve digits, so a name of its own like `t3_timing_probe` stands.
//!
//! The whole repository is walked, since the builtin root `live` and every declared root resolve from
//! it; `target/`, `node_modules/` and hidden entries hold nothing a reference reaches and are not
//! entered.

use std::collections::BTreeSet;
use std::path::Path;

use crate::{Entry, Lint, LintError, RepoContext, RepoLint, Severity};

const NAME: &str = "a-name-takes-no-led-stamp";

/// The lint.
pub struct ANameTakesNoLedStamp;

impl Lint for ANameTakesNoLedStamp {
    fn name(&self) -> &'static str {
        NAME
    }

    fn description(&self) -> &'static str {
        "no file or directory is named as a reference cites a stamped name"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

/// The paths under `rel` whose last name takes a led stamp: each directory on the way, then the
/// file.
fn taken(rel: &Path) -> Vec<(String, bool)> {
    let names: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let mut out = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let is_dir = i + 1 < names.len();
        if mockspace_refnames::entry(name, is_dir).is_some_and(|f| f.takes_a_led_stamp()) {
            out.push((names[..= i].join("/"), is_dir));
        }
    }
    out
}

impl RepoLint for ANameTakesNoLedStamp {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let view = ctx.view();
        let root = ctx.repo_root;
        let files = view.walk(root, |_, e| {
            Entry::is_dir(e)
                && (matches!(e.name(), "target" | "node_modules") || e.name().starts_with('.'))
        });
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for file in files {
            let Ok(rel) = file.strip_prefix(root) else { continue };
            for (path, is_dir) in taken(rel) {
                if !seen.insert(path.clone()) {
                    continue;
                }
                let kind = if is_dir { "directory" } else { "file" };
                let mut e = LintError::error(
                    path.clone(),
                    0,
                    NAME,
                    format!(
                        "the {kind} {path} is named as a reference cites a stamped name (`{lead}` and \
                         twelve digits), so a citation of such a name would reach it too. Rename it so its \
                         name does not open on `{lead}` and a stamp.",
                        lead = mockspace_refnames::LEAD,
                    ),
                );
                e.path = Some(path);
                out.push(e);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::RepoInput;

    fn findings(files: &[&str]) -> Vec<String> {
        let tmp = mockspace_manifest::scratch::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        for f in files {
            let p = tmp.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir_all(&mock).unwrap();
        let registry = crate::RegistryView::default();
        let no_crates = BTreeSet::new();
        let input = RepoInput {
            mock_dir:    &mock,
            repo_root:   tmp.path(),
            all_crates:  &no_crates,
            src_dirs:    &[],
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            ref_files:   &crate::src_layout::NO_REF_FILES,
            registry:    &registry,
        };
        crate::testkit::check_repo(&ANameTakesNoLedStamp, input)
            .into_iter()
            .map(|e| e.path.unwrap_or_default())
            .collect()
    }

    // Mutation: asking of the stamped name instead refuses every research file; asking only of a
    // file's name lets a directory of the led spelling stand; asking of any digits after the lead
    // refuses a probe `t4_update.sh`; walking the mock directory alone lets `docs/` stand; walking
    // `target/` refuses a build.
    #[test]
    fn a_file_or_a_directory_named_as_a_led_stamp_is_refused_and_nothing_else_is() {
        let found = findings(&[
            "mock/research/202611011713_renderer-roadmap.md",
            "mock/research/t202611011713_renderer-roadmap.md",
            "mock/research/T202611011713-notes/NOTES.md",
            "mock/research/sketches/t202611011713_probe/FINDINGS.md",
            "mock/research/202611120700_x/008_probes/t4_update.sh",
            "mock/registry/ticket/items/t3_timing_probe.toml",
            "mock/research/tests.md",
            "docs/202611011713_x.md",
            "docs/t202611011713_x.md",
            "target/t202611011713/out.md",
            "node_modules/t202611011713/x.js",
            ".git/t202611011713",
        ]);
        assert_eq!(found, [
            "docs/t202611011713_x.md",
            "mock/research/T202611011713-notes",
            "mock/research/sketches/t202611011713_probe",
            "mock/research/t202611011713_renderer-roadmap.md",
        ]);
    }

    // Mutation: one finding per file under a refused directory buries the one rename it needs.
    #[test]
    fn a_refused_directory_is_named_once_whatever_it_holds() {
        assert_eq!(findings(&["t202611011713/a.md", "t202611011713/b.md"]), [
            "t202611011713"
        ]);
    }
}
