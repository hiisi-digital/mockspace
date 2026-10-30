//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Holding a lint to the reach it declared.
//!
//! A declared [`Reach`] is a promise that the lint reads nothing outside it, and the engine acts on
//! the promise by not running the lint when only something outside moved. A lint that reads past
//! its declaration is then not slow but wrong: it would replay a stale verdict. So the promise is
//! tested the one way a promise about reading can be, by moving what the lint was promised not to
//! read and requiring the same findings.
//!
//! The check takes real source as its corpus, so a lint that reads past its reach only when some
//! construct is present is caught when the corpus has the construct. It is a sample, and says so:
//! a pass means the lint gave the same findings on this corpus, not that it cannot read outside.
//! A pack's tests call [`crate_lint`] and [`workspace_lint`] on their own lints with the corpus
//! their lints hunt in.
//!
//! What moves, by declared reach:
//!
//! - **File**: every other file in the crate, the design and doc templates, the SHAME template,
//!   the dependency list, the workspace's crate names, and the directory behind `workspace_root`.
//!   Held fixed: the file's text and the crate's scalars ([`crate::verdict::file_scalars`]).
//! - **Crate**: the set of crates the workspace has, other crates' primitive introductions, and the
//!   directory behind `workspace_root`.
//! - **Workspace**: the directory behind `workspace_root`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::testkit::LintFixture;
use crate::verdict::encode;
use crate::{CrateLint, CrateSourceFile, LintContext, LintError, Reach, WorkspaceLint};

/// Two directories a lint could be made to read through `workspace_root`: one with content a lint
/// might look for, one without. The findings must not depend on which.
pub struct Roots<'a> {
    /// A directory shaped like a repository: a config file, a `design_rounds/` directory, crates.
    pub populated: &'a Path,
    /// An empty directory.
    pub empty:     &'a Path,
}

/// The files of `corpus` as a crate's sources, `src/lib.rs` first.
fn sources(corpus: &[(&str, &str)]) -> Vec<CrateSourceFile> {
    corpus
        .iter()
        .map(|(rel, text)| CrateSourceFile {
            rel_path: PathBuf::from(rel),
            text:     (*text).to_string(),
        })
        .collect()
}

fn same(a: &[LintError], b: &[LintError]) -> bool {
    encode(a) == encode(b)
}

/// Check a crate lint's declared reach against `corpus`, a list of `(crate-relative path, text)`
/// whose first entry is `src/lib.rs`.
///
/// # Errors
///
/// A description of the first input outside the declared reach whose movement changed the
/// findings. `Ok` for a lint declaring a reach the engine does not save.
pub fn crate_lint(lint: &dyn CrateLint, corpus: &[(&str, &str)], roots: &Roots) -> Result<(), String> {
    let reach = lint.reach();
    if !reach.is_cacheable() || reach == Reach::Workspace {
        return Ok(());
    }
    if reach == Reach::File && !lint.per_file() {
        return Err(format!(
            "`{}` declares a file reach but judges the whole crate (`per_file` is false), so its \
             verdict can depend on every file; declare `Reach::Crate`",
            lint.name()
        ));
    }
    let all = sources(corpus);
    let joined: String = corpus.iter().map(|(_, t)| *t).collect::<Vec<_>>().join("\n");
    let many: Vec<String> = (0 .. 8).map(|i| format!("sibling-{i}")).collect();

    // The crate's own inputs, the same in both runs for a crate-reach lint.
    let rich_of = |text: &str| {
        LintFixture::new(text)
            .with_sources(all.clone())
            .with_design_doc(&joined)
            .with_doc_content(&joined)
            .with_shame_doc(&joined)
            .with_deps(&["dep-a", "dep-b"])
            .with_all_crates(&many.iter().map(String::as_str).collect::<Vec<_>>())
            .with_workspace_root(roots.populated)
    };

    match reach {
        Reach::File => {
            for (rel, text) in corpus {
                let rich = rich_of(text);
                let poor = LintFixture::new(text)
                    .with_sources(vec![CrateSourceFile {
                        rel_path: PathBuf::from(rel),
                        text:     (*text).to_string(),
                    }])
                    .with_workspace_root(roots.empty);
                let (a, b) = (lint.check(&rich.ctx()), lint.check(&poor.ctx()));
                if !same(&a, &b) {
                    return Err(format!(
                        "`{}` declares a file reach, but its findings on `{rel}` changed when \
                         only what is outside the file moved: with the crate's other files, \
                         documents, dependencies and workspace present it found {} and without \
                         them {}. It reads past the file; declare a wider reach",
                        lint.name(),
                        a.len(),
                        b.len(),
                    ));
                }
            }
        },
        Reach::Crate => {
            let root_text = corpus[0].1;
            let near = rich_of(root_text);
            let far = LintFixture::new(root_text)
                .with_sources(all.clone())
                .with_design_doc(&joined)
                .with_doc_content(&joined)
                .with_shame_doc(&joined)
                .with_deps(&["dep-a", "dep-b"])
                .with_all_crates(&many.iter().map(String::as_str).collect::<Vec<_>>())
                .with_workspace_root(roots.empty)
                .with_primitive_introductions(BTreeMap::from([(
                    "some-other-crate".to_string(),
                    vec!["u8".to_string()],
                )]));
            let (a, b) = (lint.check(&near.ctx()), lint.check(&far.ctx()));
            if !same(&a, &b) {
                return Err(format!(
                    "`{}` declares a crate reach, but its findings changed when only what is \
                     outside the crate moved (the directory behind `workspace_root`, another \
                     crate's primitive introductions): {} against {}. It reads past the crate; \
                     declare `Reach::Repository`",
                    lint.name(),
                    a.len(),
                    b.len(),
                ));
            }
        },
        Reach::Workspace | Reach::Repository | Reach::Change => {},
    }
    Ok(())
}

/// Check a workspace lint's declared reach: it is handed `corpus` as two crates, and the directory
/// behind `workspace_root` is the only thing that moves.
///
/// # Errors
///
/// As [`crate_lint`].
pub fn workspace_lint(
    lint: &dyn WorkspaceLint,
    corpus: &[(&str, &str)],
    roots: &Roots,
) -> Result<(), String> {
    if lint.reach() != Reach::Workspace {
        return Ok(());
    }
    let all = sources(corpus);
    let names = ["alpha", "beta"];
    let run = |root: &Path| -> Vec<LintError> {
        let fixtures: Vec<LintFixture> = names
            .iter()
            .map(|n| {
                LintFixture::new(corpus[0].1)
                    .with_crate_name(n, n)
                    .with_sources(all.clone())
                    .with_all_crates(&names)
                    .with_workspace_root(root)
            })
            .collect();
        let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
        let pairs: Vec<(&str, &LintContext)> = names.iter().copied().zip(ctxs.iter()).collect();
        lint.check_all(&pairs)
    };
    let (a, b) = (run(roots.populated), run(roots.empty));
    if same(&a, &b) {
        Ok(())
    } else {
        Err(format!(
            "`{}` declares a workspace reach, but its findings changed when only the directory \
             behind `workspace_root` moved: {} against {}. It reads the repository; declare \
             `Reach::Repository`",
            lint.name(),
            a.len(),
            b.len(),
        ))
    }
}
