//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Handing a crate lint its files: once per file, or once per crate, with saved verdicts.
//!
//! Moved out of `lib.rs` with the lazy parse and the file-level cache it gained, which the
//! file-size limit made necessary: the dispatcher was a hundred lines of its own concern.

use std::cell::{OnceCell, RefCell};

use tree_sitter::Tree;

use crate::path_filter::PathFilter;
use crate::verdict::{self, VerdictCache};
use crate::{CrateLint, CrateSourceFile, LintContext, LintError, make_parser, timing};

/// A crate's source files, each parsed the first time something needs its tree.
///
/// Eager parsing priced every gate run at one parse per file whether or not any lint had work to
/// do. With saved verdicts most runs have none: a file whose text and configuration are unchanged
/// is answered from the store and its tree is never built. A file that will not parse is dropped
/// rather than reported; the compiler says it better and says it first.
pub(crate) struct Parsed<'a> {
    files:  &'a [CrateSourceFile],
    trees:  Vec<OnceCell<Option<Tree>>>,
    parser: RefCell<tree_sitter::Parser>,
}

impl<'a> Parsed<'a> {
    pub(crate) fn new(files: &'a [CrateSourceFile]) -> Self {
        Self {
            files,
            trees: files.iter().map(|_| OnceCell::new()).collect(),
            parser: RefCell::new(make_parser()),
        }
    }

    /// The tree of file `i`, parsed now if nobody has asked for it yet. `None` for a file that
    /// does not parse.
    fn tree(&self, i: usize) -> Option<&Tree> {
        self.trees[i]
            .get_or_init(|| {
                timing::time("parse:crate-files", || {
                    self.parser.borrow_mut().parse(&self.files[i].text, None)
                })
            })
            .as_ref()
    }
}

/// Parse every source file once, for the per-file dispatch to share. Lazily: see [`Parsed`].
pub(crate) fn parse_sources(sources: &[CrateSourceFile]) -> Parsed<'_> {
    Parsed::new(sources)
}

/// What a file-reach lint's saved verdict is keyed on besides the file's text.
pub(crate) struct FileCache<'c> {
    pub(crate) cache:     &'c dyn VerdictCache,
    /// [`verdict::behaviour_material`] for the lint.
    pub(crate) behaviour: String,
    /// [`verdict::file_scalars`] for the crate.
    pub(crate) scalars:   String,
}

/// Run a lint over every source file in the crate, or once over the crate when
/// it declared itself crate-scoped.
///
/// This exists because `LintContext::source` is the crate root and nothing else,
/// so a lint reading it saw `src/lib.rs` and skipped every module file. In one crate
/// that was 76 of 101 public items; the gate reported clean while most of the
/// surface was never looked at. `file_size` had already been fixed by growing
/// its own loop over `all_sources`, which is the same fix twenty-six more times
/// and a fresh tree-sitter parse inside each lint.
///
/// Doing it here instead means no lint changes at all. The caller parses each
/// file once and every per-file lint shares the trees; the parse used to live
/// inside this function, which priced a crate at one parse per file per lint,
/// a loop on the wrong side of the work.
/// # Where the path filter gets applied, and why to both dispatch shapes
///
/// The filter has to reach the crate-scoped case as well as the per-file one, or it would
/// be a rule any lint could step around by declaring `per_file = false` and walking
/// `all_sources` for itself. So a filtered crate-scoped lint gets a context whose
/// `all_sources` holds only what it may see, and whose `source` and `tree` point at the
/// first surviving file rather than at a crate root it was configured not to look at.
///
/// That costs a clone of each surviving file, paid only by a lint someone actually
/// configured a filter for: [`crate::LintConfig::filter_for`] hands back `None` for every other
/// lint and this takes the borrowed path.
///
/// A lint whose filter admits nothing is skipped rather than run against an empty crate,
/// since "nothing to look at" and "looked and found nothing" are different answers and
/// something like `no-empty-crate` reports on the second.
pub(crate) fn check_every_file(
    lint: &dyn CrateLint,
    ctx: &LintContext,
    parsed: &Parsed,
    filter: Option<&PathFilter>,
) -> Vec<LintError> {
    check_every_file_saving(lint, ctx, parsed, filter, None)
}

/// [`check_every_file`], answering a per-file lint from saved verdicts where `saved` is given.
pub(crate) fn check_every_file_saving(
    lint: &dyn CrateLint,
    ctx: &LintContext,
    parsed: &Parsed,
    filter: Option<&PathFilter>,
    saved: Option<&FileCache>,
) -> Vec<LintError> {
    // Indices rather than the files, so a tree is asked for only where it is needed.
    let kept: Vec<usize> = (0 .. parsed.files.len())
        .filter(|&i| filter.is_none_or(|f| f.allows(&parsed.files[i].rel_path)))
        .collect();

    // A filter admitting no file leaves the lint nothing to look at, which is not a pass.
    if filter.is_some() && kept.is_empty() {
        return Vec::new();
    }

    if !lint.per_file() {
        // an unfiltered crate-scoped lint gets the context untouched, which is the path
        // every lint took before filters existed
        let Some(f) = filter else {
            return lint.check(ctx);
        };
        let owned: Vec<CrateSourceFile> = ctx
            .all_sources
            .iter()
            .filter(|s| f.allows(&s.rel_path))
            .cloned()
            .collect();
        if owned.is_empty() {
            return Vec::new();
        }
        // the first surviving file that parses is the root the lint is shown
        let Some((root, root_tree)) = kept
            .iter()
            .find_map(|&i| parsed.tree(i).map(|t| (&parsed.files[i], t)))
        else {
            return Vec::new();
        };
        return lint.check(&LintContext {
            source: &root.text,
            tree: root_tree,
            all_sources: &owned,
            ..*ctx
        });
    }

    let mut errors = Vec::new();
    let mut any_judged = false;
    for i in kept {
        let file = &parsed.files[i];
        let found = match saved {
            Some(s) => {
                let key = s.cache.digest(&[
                    b"file",
                    s.behaviour.as_bytes(),
                    s.scalars.as_bytes(),
                    file.text.as_bytes(),
                ]);
                match s.cache.replay(&key).and_then(|t| verdict::decode(&t)) {
                    Some(found) => {
                        s.cache.tally(true, lint.name());
                        any_judged = true;
                        found
                    },
                    None => {
                        let Some(tree) = parsed.tree(i) else { continue };
                        any_judged = true;
                        s.cache.tally(false, lint.name());
                        let found = lint.check(&LintContext {
                            source: &file.text,
                            tree,
                            ..*ctx
                        });
                        s.cache.remember(&key, &verdict::encode(&found));
                        found
                    },
                }
            },
            None => {
                let Some(tree) = parsed.tree(i) else { continue };
                any_judged = true;
                lint.check(&LintContext {
                    source: &file.text,
                    tree,
                    ..*ctx
                })
            },
        };
        let path = file.rel_path.to_string_lossy();
        for mut err in found {
            // The crate root stays bare, because that is where a reader
            // already looks. Anything else carries its file as a field, and
            // the renderer turns it into a location that points at something;
            // folded into the message it made `{crate}:{line}` read as a
            // location that pointed at nothing.
            if path != "src/lib.rs" {
                err.path = Some(path.to_string());
            }
            errors.push(err);
        }
    }

    // An unfiltered crate where no file parsed means a caller built the context without
    // `all_sources`, so the old behaviour is what it expects rather than no coverage at all.
    if !any_judged && filter.is_none() {
        return lint.check(ctx);
    }
    errors
}
