//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Lint: no empty crates.
//!
//! Flags crates whose `src/**/*.rs` files contain only doc comments, use
//! statements, and/or `mod` declarations but no type definitions, trait
//! definitions, macro invocations, or function definitions.
//!
//! The crate is judged by the union of all its `.rs` source files, not only
//! `lib.rs`. A crate whose `lib.rs` only does `pub mod x; pub use x::*;`
//! (the common re-export pattern) is NOT empty as long as `x.rs` defines
//! something substantive.
//!
//! Pre-commit: WARNING. Pre-push / build: ERROR.
//! (Severity is WARNING so it doesn't block incremental work, but the crate
//! must be populated before pushing.)
//!
//! A crate a design round is adding is not empty yet, it is unborn. While a
//! round is open and its doc changelist names the crate, the crate's
//! documentation alone passes: the doc phase commits the manifest and a
//! `lib.rs` holding only the crate's docs, which `changelist-required` admits
//! and cargo needs to read the workspace, and the items come in the source
//! phase. Once both changelists are locked the lint holds the crate again, so
//! a round cannot close over a crate it never populated. A crate HEAD holds
//! source for is never shielded, so an existing crate gutted to its docs is
//! empty whatever the changelist says about it.

use std::path::Path;

use tree_sitter::Node;

use crate::changelist_helpers::{self, Phase};
use crate::{CrateLint, Lint, LintContext, LintError, View};

const LINT_NAME: &str = "no-empty-crate";

/// Node kinds that count as "substantive content": having at least one means
/// the crate is not empty.
const SUBSTANTIVE_KINDS: &[&str] = &[
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "function_item",
    "const_item",
    "static_item",
    "type_item",
    "impl_item",
    "macro_invocation",
    "macro_definition",
];

pub struct NoEmptyCrate;

impl Lint for NoEmptyCrate {
    /// Crate-scoped. Whether a crate is empty is a fact about the crate; per
    /// file it would call every module file an empty crate.
    fn per_file(&self) -> bool {
        false
    }

    fn default_severity(&self) -> crate::Severity {
        crate::Severity::ADVISORY
    }

    fn name(&self) -> &'static str {
        LINT_NAME
    }
}

impl CrateLint for NoEmptyCrate {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        // lib.rs tree is already parsed: check it first so the common case
        // of a populated lib.rs avoids re-parsing every module.
        if has_substantive_content(ctx.tree().root_node()) {
            return Vec::new();
        }

        // Fall through: scan every other source file (e.g. `src/foo.rs`) for
        // substantive content. Handles the `pub mod foo; pub use foo::*;`
        // re-export pattern where lib.rs itself holds nothing substantive
        // but the crate clearly has an API surface.
        for file in ctx.all_sources() {
            // Skip lib.rs: already checked via ctx.tree() above.
            if file.rel_path.file_name().and_then(|n| n.to_str()) == Some("lib.rs") {
                continue;
            }
            let tree = file.tree();
            if has_substantive_content(tree.root_node()) {
                return Vec::new();
            }
        }

        if born_in_the_open_round(ctx.view(), ctx.workspace_root(), ctx.crate_name) {
            return Vec::new();
        }

        vec![LintError::warning(
            ctx.crate_name.to_string(),
            1,
            LINT_NAME,
            format!(
                "crate `{}` has no type, trait, function, or macro definitions: \
                 populate it or remove it from the workspace",
                ctx.crate_name,
            ),
        )]
    }
}

/// Whether the round open under `mock_dir` is adding `crate_name`: HEAD holds
/// no source for it, its phase is DOC, DRAFT or IMPL, and its doc changelist
/// names the crate. An existing crate the changelist mentions is not shielded.
fn born_in_the_open_round(view: &View<'_>, mock_dir: &Path, crate_name: &str) -> bool {
    if head_holds_source(view, mock_dir, crate_name) {
        return false;
    }
    let rounds = mock_dir.join("design_rounds");
    if !matches!(
        changelist_helpers::current_phase(view, &rounds),
        Phase::Doc | Phase::SrcPlan | Phase::Src
    ) {
        return false;
    }
    changelist_helpers::find_active_doc_cl(view, &rounds)
        .or_else(|| changelist_helpers::find_locked_doc_cl(view, &rounds))
        .and_then(|cl| view.text(rounds.join(cl.filename)))
        .is_some_and(|text| names_crate(text, crate_name))
}

/// Whether `HEAD` holds source for `crate_name`: a manifest under `mock_dir`
/// naming the package, whose `src` holds a file with substantive content.
/// The doc phase of the round adding the crate commits its manifest and a
/// docs-only `lib.rs`, so the crate being in `HEAD` is not enough. A query
/// that fails proves nothing, so it answers yes.
fn head_holds_source(view: &View<'_>, mock_dir: &Path, crate_name: &str) -> bool {
    let pattern = format!(r#"^name[[:space:]]*=[[:space:]]*"{crate_name}"[[:space:]]*$"#);
    let Some(grep) = git_out(view, mock_dir, &["grep", "-l", "-E", &pattern, "HEAD", "--", "*Cargo.toml"])
    else {
        return true;
    };
    let manifests = match grep {
        Ok(out) => out,
        Err(1) => return false, // no manifest names it: a crate HEAD does not hold
        Err(_) => return true,
    };
    for manifest in manifests.lines().filter_map(|l| l.strip_prefix("HEAD:")) {
        let src = Path::new(manifest).parent().unwrap_or(Path::new("")).join("src");
        let src = src.to_string_lossy();
        let Some(Ok(files)) = git_out(view, mock_dir, &["ls-tree", "-r", "--name-only", "HEAD", "--", &src])
        else {
            return true;
        };
        for file in files.lines().filter(|f| f.ends_with(".rs")) {
            let Some(Ok(text)) = git_out(view, mock_dir, &["show", &format!("HEAD:./{file}")]) else {
                return true;
            };
            if has_substantive_content(view.parse(&text).root_node()) {
                return true;
            }
        }
    }
    false
}

/// Ask git in `dir` through the view: `None` when it cannot start, `Err(code)`
/// on a failing exit.
fn git_out(view: &View<'_>, dir: &Path, args: &[&str]) -> Option<Result<String, i32>> {
    let out = view.git_in(dir, args);
    if !out.started() {
        return None;
    }
    if out.ok() {
        Some(Ok(String::from_utf8_lossy(out.stdout()).into_owned()))
    } else {
        Some(Err(out.code().unwrap_or(-1)))
    }
}

/// Whether `text` names `crate_name` as a whole word, so `foo` is not named by
/// a mention of `foo-extra`.
fn names_crate(text: &str, crate_name: &str) -> bool {
    let part_of_a_name = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    text.match_indices(crate_name).any(|(at, _)| {
        let before = text[.. at].chars().next_back();
        let after = text[at + crate_name.len() ..].chars().next();
        !before.is_some_and(part_of_a_name) && !after.is_some_and(part_of_a_name)
    })
}

fn has_substantive_content(node: Node) -> bool {
    if SUBSTANTIVE_KINDS.contains(&node.kind()) {
        return true;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_substantive_content(child) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::testkit::LintFixture;

    fn make_ctx(lib_src: &str, extra: &[(&str, &str)]) -> LintFixture {
        extra
            .iter()
            .fold(LintFixture::new(lib_src).with_crate_name("test-crate", "test"), |f, (name, body)| {
                f.with_module(&format!("src/{name}"), body)
            })
    }

    /// [`make_ctx`] with the mock dir at `mock_dir`, where a round and a
    /// repository can stand.
    fn make_ctx_at(mock_dir: &std::path::Path, lib_src: &str, extra: &[(&str, &str)]) -> LintFixture {
        make_ctx(lib_src, extra).with_workspace_root(mock_dir)
    }

    #[test]
    fn populated_lib_rs_passes() {
        let fixture = make_ctx("pub struct Foo;", &[]);
        let ctx = fixture.ctx();
        assert!(NoEmptyCrate.check(&ctx).is_empty());
    }

    #[test]
    fn reexport_from_submodule_passes() {
        // lib.rs only contains mod declarations and re-exports: no direct
        // substantive content, but the submodule provides it. This is the
        // common `pub mod foo; pub use foo::*;` pattern the old lint
        // flagged as a false positive.
        let lib = "pub mod inner;\npub use inner::Thing;\n";
        let inner = "pub struct Thing;\n";
        let fixture = make_ctx(lib, &[("inner.rs", inner)]);
        let ctx = fixture.ctx();
        assert!(NoEmptyCrate.check(&ctx).is_empty());
    }

    #[test]
    fn truly_empty_crate_still_flags() {
        // lib.rs and the only submodule are both empty shells.
        let lib = "//! only a doc comment\n";
        let inner = "//! nothing here either\n";
        let fixture = make_ctx(lib, &[("inner.rs", inner)]);
        let ctx = fixture.ctx();
        let errs = NoEmptyCrate.check(&ctx);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn lib_rs_only_with_docs_flags() {
        let fixture = make_ctx("//! just docs\n", &[]);
        let ctx = fixture.ctx();
        assert_eq!(NoEmptyCrate.check(&ctx).len(), 1);
    }

    /// A mock dir whose round holds the changelists `names`, the doc one
    /// carrying `doc_text`.
    fn round(tag: &str, names: &[&str], doc_text: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        for name in names {
            let text = if name.contains(".doc.") { doc_text } else { "x\n" };
            std::fs::write(root.join("design_rounds").join(name), text).unwrap();
        }
        commit(&root);
        root
    }

    /// Commit everything under `root`, making it a repository first.
    fn commit(root: &std::path::Path) {
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git").args(args).current_dir(root).output().unwrap();
            assert!(ok.status.success(), "git {args:?}");
        };
        if !root.join(".git").exists() {
            run(&["init", "-q"]);
            run(&["config", "user.email", "t@example.com"]);
            run(&["config", "user.name", "t"]);
        }
        run(&["add", "-A"]);
        run(&["commit", "-q", "--allow-empty", "-m", "seed", "--no-gpg-sign"]);
    }

    /// Put crate `test-crate` in `HEAD` with `lib` as its `lib.rs`.
    fn with_crate_at_head(root: PathBuf, lib: &str) -> PathBuf {
        let dir = root.join("crates/test-crate");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"test-crate\"\n").unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib).unwrap();
        commit(&root);
        root
    }

    const DOC_OPEN: &[&str] = &["202601010000_changelist.doc.md"];
    const NAMES_IT: &str = "## Changes\n\nA new crate, `test-crate`, holds the thing.\n";

    fn findings_in(root: PathBuf) -> usize {
        let fixture = make_ctx_at(&root, "//! The crate's documentation.\n", &[]);
        let n = NoEmptyCrate.check(&fixture.ctx()).len();
        let _ = std::fs::remove_dir_all(&root);
        n
    }

    /// The doc phase of a round adding the crate commits its documentation
    /// alone, and the crate is unborn rather than empty.
    #[test]
    fn a_crate_the_open_round_adds_passes_on_its_documentation() {
        assert_eq!(findings_in(round("doc", DOC_OPEN, NAMES_IT)), 0);
    }

    /// Through DRAFT and IMPL as well, since the items land during IMPL.
    #[test]
    fn a_crate_the_round_adds_passes_until_the_source_changelist_locks() {
        let draft = ["202601010000_changelist.doc.lock.md"];
        let imp = ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.md"];
        assert_eq!(findings_in(round("draft", &draft, NAMES_IT)), 0);
        assert_eq!(findings_in(round("impl", &imp, NAMES_IT)), 0);
    }

    /// The control: the same open round, its changelist not naming the crate.
    #[test]
    fn a_round_that_does_not_name_the_crate_does_not_shield_it() {
        let text = "## Changes\n\nNothing about crates.\n";
        assert_eq!(findings_in(round("other", DOC_OPEN, text)), 1);
    }

    /// A name that only contains the crate's name is another crate.
    #[test]
    fn a_longer_name_holding_the_crates_name_is_not_the_crate() {
        let text = "## Changes\n\nA new crate, `test-crate-extra`.\n";
        assert_eq!(findings_in(round("longer", DOC_OPEN, text)), 1);
    }

    /// With both changelists locked the round is closing, and a crate it never
    /// populated is empty.
    #[test]
    fn a_round_with_both_changelists_locked_holds_the_crate_again() {
        let done = ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.lock.md"];
        assert_eq!(findings_in(round("done", &done, NAMES_IT)), 1);
    }

    /// An existing crate the changelist names, gutted to its docs, is empty:
    /// the round is not adding it.
    #[test]
    fn an_existing_crate_the_round_names_gutted_to_docs_is_empty() {
        let root = with_crate_at_head(round("existing", DOC_OPEN, NAMES_IT), "pub fn a() {}\n");
        assert_eq!(findings_in(root), 1);
    }

    /// The crate the round scaffolded in an earlier commit, docs only in `HEAD`,
    /// is still being added.
    #[test]
    fn a_crate_the_round_scaffolded_earlier_is_still_shielded() {
        let root = with_crate_at_head(round("earlier", DOC_OPEN, NAMES_IT), "//! Docs.\n");
        assert_eq!(findings_in(root), 0);
    }
}
