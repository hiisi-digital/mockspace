//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Runtime custom-lint loading via a project-local lint cdylib.
//!
//! Under the dissolved-proxy model the engine binary is shared across every
//! repo pinned to one mockspace version, so a repo's own lints can no longer
//! be statically compiled into it. Instead the engine builds the repo's lints
//! into a small cdylib and dlopens it at run time:
//!
//! - the repo's own `mock/lints/*.rs` compile into the cdylib, whose generated
//!   crate lives outside the worktree, in the shared store (`store`), at a
//!   path keyed by the repo's identity and the revs it builds against, so
//!   every worktree of one repo at the same revs is the same cargo package and
//!   shares its dependency builds. The store is under `CARGO_TARGET_DIR` when
//!   one is set and under `$XDG_CACHE_HOME/mockspace/lints` otherwise. Only
//!   `<mock>/target/mockspace-lints/shared-dir` stays in the worktree,
//!   pointing at it. `mock clean` leaves the store alone; the engine removes
//!   the builds nothing can come back to;
//! - `[lint-crates]` pack dependencies resolve through cargo's normal global
//!   cache and link in;
//! - the cdylib exports one `extern "C"` collector that hands back the boxed
//!   lints, which the engine feeds to `run_with_custom_lints`.
//!
//! ABI: `Box<dyn Lint>` crosses the cdylib boundary. This is sound only when
//! both sides resolve `mockspace-lint-rules` at the *same* pin AND build with
//! the same toolchain, so the trait's vtable layout is identical. Two things
//! back that: the launcher passes the pin-matched dep spec (`lint_rules_dep`,
//! the same version it built the engine from), and it folds `rustc -vV` into
//! the engine's cache key, so a toolchain change re-keys and rebuilds the
//! engine rather than pairing a frozen binary with a differently-compiled
//! cdylib. Validated experimentally (identical trait source compiled
//! separately still dispatches correctly).
//!
//! FIXME: one residual is unenforced. The cached engine is built by `cargo
//! install --git mockspace` under mockspace's own `rust-toolchain.toml`, while
//! this cdylib is built by `cargo build` under the consumer repo's toolchain.
//! They match today because the whole workspace pins one nightly, but a repo
//! pinning a different toolchain than the engine's would desync the vtable
//! layout with no signal. The complete fix builds the cdylib under the engine's
//! toolchain (capture it at engine-build time, pin it in the generated crate).
//! Tracked as a follow-up.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mockspace_lint_rules::LintPack;
use mockspace_lint_rules::tool::duplicate_tool_names;

use crate::bootstrap::{
    ToolCrate,
    discover_custom_lint_files,
    discover_tool_crates,
    parse_lint_crates,
    scan_lint_functions,
};
use crate::config::Config;
pub(crate) use store::ScopedPlan;

pub(crate) mod store;
mod build;
mod built;
mod depinfo;
mod identity;

/// The `extern "C"` symbol the generated cdylib exports.
const COLLECT_SYMBOL: &[u8] = b"__mockspace_collect_lints";

/// Lints loaded from a repo's cdylib, plus the library keeping their vtables
/// alive. Dropping this frees the lints, then unloads the library, in that
/// order (the field order matters: `pack` must drop before `_lib`).
pub struct LoadedLints {
    pub pack: LintPack,
    _lib:     libloading::Library,
}

/// What a repo declares for the cdylib to carry.
struct Content {
    lints_dir:  PathBuf,
    lint_files: Vec<String>,
    packs:      Vec<(String, String)>,
    tools:      Vec<ToolCrate>,
}

/// The repo's custom content, or `None` when it declares no `mock/lints/*.rs`,
/// no `[lint-crates]` and no tool (the common case, and a fast path).
fn discover(cfg: &Config, config_path: &Path) -> Option<Content> {
    let lints_dir = cfg.mock_dir.join("lints");
    let lint_files = discover_custom_lint_files(&lints_dir);
    let packs = parse_lint_crates(config_path);
    // A repo whose only custom content is a tool still needs the cdylib built,
    // and this early return is what would silently skip it: `mock <tool>` would
    // report an unknown subcommand in a repo that plainly declares one.
    let tools = discover_tool_crates(&cfg.mock_dir.join("tools"));
    if lint_files.is_empty() && packs.is_empty() && tools.is_empty() {
        return None;
    }
    Some(Content {
        lints_dir,
        lint_files,
        packs,
        tools,
    })
}

/// What a run of the one tool `tool` does: refuse it, take the whole build, or build the tool alone, from the
/// record the last whole build left.
pub(crate) fn scoped_plan(cfg: &Config, tool: &str) -> store::ScopedPlan {
    store::plan_scoped(store::duplicates_of(&cfg.mock_dir), tool)
}

/// The content of a build cut to the one tool in `<mock>/tools/<tool>`: that
/// crate and its own path dependencies, no lint file and no pack. `None` when
/// `tool` is no tool directory, so the caller falls back to the whole build.
///
/// A tool is handed the repo's paths and its arguments and nothing of the lint
/// pack (`ToolContext`), so the lints and the packs add nothing to a run of
/// one, and compiling them is what a run of it waits on.
///
/// Leaving the packs out means a name a pack also registers is not met here. The
/// last whole build recorded such names (`store::record_duplicates`), and
/// `scoped_plan` refuses a run of one of them with exit 2 before this is called,
/// or takes the whole build once when nothing is recorded yet.
fn discover_scoped(cfg: &Config, config_path: &Path, tool: &str) -> Option<Content> {
    let mut content = discover(cfg, config_path)?;
    content.tools.retain(|t| t.dir == tool);
    if content.tools.is_empty() {
        return None;
    }
    content.lint_files.clear();
    content.packs.clear();
    Some(content)
}

/// [`load`] cut to the one tool `tool`, falling back to the whole build where
/// `tool` names no tool directory.
pub fn load_tool(
    cfg: &Config,
    config_path: &Path,
    lint_rules_dep: &str,
    tool: &str,
) -> Result<Option<LoadedLints>, String> {
    let Some(content) = discover_scoped(cfg, config_path, tool) else {
        return load(cfg, config_path, lint_rules_dep);
    };
    let base = store::shared_base(&cfg.repo_root)?;
    build_and_load(&base, cfg, &content, lint_rules_dep, Some(tool))
}

/// Build and load this repo's custom lints, if it has any. Returns `None` when
/// the repo declares none. `lint_rules_dep` is the cargo dependency *value*
/// for `mockspace-lint-rules`, pin-matched to the running engine, e.g.
/// `{ package = "mockspace-lint-rules", git = "...", rev = "..." }`.
///
/// The generated crate goes in the machine's shared store, see
/// [`store::shared_base`].
pub fn load(
    cfg: &Config,
    config_path: &Path,
    lint_rules_dep: &str,
) -> Result<Option<LoadedLints>, String> {
    let Some(content) = discover(cfg, config_path) else {
        return Ok(None);
    };
    let base = store::shared_base(&cfg.repo_root)?;
    build_and_load(&base, cfg, &content, lint_rules_dep, None)
}

/// [`load`] against a store at `base`, for a caller that owns where the
/// generated crate goes. The tests use it to keep every build, which is about
/// 85 MB, inside a directory they delete.
pub fn load_in(
    base: &Path,
    cfg: &Config,
    config_path: &Path,
    lint_rules_dep: &str,
) -> Result<Option<LoadedLints>, String> {
    match discover(cfg, config_path) {
        Some(content) => build_and_load(base, cfg, &content, lint_rules_dep, None),
        None => Ok(None),
    }
}

fn build_and_load(
    base: &Path,
    cfg: &Config,
    content: &Content,
    lint_rules_dep: &str,
    scope: Option<&str>,
) -> Result<Option<LoadedLints>, String> {
    // The generated crate lives outside the worktree, at a path keyed by the
    // repo's identity and the revs it builds against, so every worktree of one
    // repo at the same revs resolves to the same cargo package and shares one
    // build. See `store::lints_manifest_path`. A branch pack is resolved to its
    // tip once, here, and both the key and the manifest are made from that.
    std::fs::create_dir_all(base)
        .map_err(|e| format!("could not create {}: {e}", base.display()))?;
    let packs = store::resolve_packs(
        &content.packs,
        &base.join(".pack-pins"),
        &crate::pack_pin::ls_remote_head,
    );
    let manifest = store::lints_manifest_path(base, cfg, lint_rules_dep, &packs, scope);
    let gen_dir = manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("no directory for {}", manifest.display()))?;
    crate::build_dir::mark(base);
    // One crate, several worktrees: each run rewrites it to point at its own
    // lint sources, so the write, the build and the load are one critical
    // section.
    let lock = store::lock_dir(&gen_dir)?;
    write_cdylib_crate(
        &gen_dir,
        &content.lints_dir,
        &content.lint_files,
        &packs,
        &content.tools,
        lint_rules_dep,
        cfg,
    )?;
    store::record_owner(&gen_dir, &cfg.mock_dir, scope)?;
    store::touch_used(&gen_dir)?;
    #[cfg(test)]
    store::knobs::pause_between_write_and_build(&gen_dir);
    // The library is kept under an identity made from its inputs, so a tree of the same content
    // loads a peer's build and nothing is built or relinked on its account. Where the inputs
    // cannot be read, it is built as ever and identified by its bytes.
    let identity = mockspace_lint_rules::timing::time("phase:pack-identity", || {
        pack_identity(&gen_dir, content, cfg)
    });
    #[cfg(test)]
    store::knobs::run_after_identity(&gen_dir);
    let crate_name = gen_crate_name(&gen_dir);
    let library = match &identity {
        Some(id) => built::dylib_for(base, &id.key, &id.roots, &gen_dir, depinfo::TRUST_MARGIN, || {
            #[cfg(test)]
            store::knobs::note_build(&gen_dir);
            build::build_cdylib(&gen_dir, &crate_name, &id.roots.remaps())
        }),
        None => build::build_cdylib(&gen_dir, &crate_name, &[])
            .map(|b| built::Library { path: b.dylib, identity: None }),
    };
    let (built, identity) = match library {
        Ok(l) => (Ok(l.path), l.identity),
        Err(e) => (Err(e), None),
    };

    // SAFETY: the cdylib is our own generated crate, built moments ago from
    // this repo's lint sources against the same pinned mockspace-lint-rules
    // the engine uses, so the `Lint`/`CrossCrateLint` vtable layout matches.
    // The returned boxes borrow their vtables from `lib`, so `lib` is stored
    // in `LoadedLints` and outlives them.
    let loaded = built.and_then(|dylib| unsafe { collect(&dylib, identity) }.map(Some));
    // Every whole build leaves the names its packs and local tools register twice, which a run of one tool
    // cannot ask for without building the packs.
    if scope.is_none() {
        if let Ok(Some(l)) = &loaded {
            store::record_duplicates(&cfg.mock_dir, &duplicate_tool_names(&l.pack.tools))?;
        }
    }
    // Released only once the library is mapped. The mapping outlives a later
    // rebuild by another worktree, and holding the lock for the whole lint run
    // would serialise every worktree on it.
    drop(lock);
    // After the lock, so removing a dead build does not hold up this run's
    // peers, and for every outcome, so a failed build still clears the dead.
    let removed = store::reap(
        base,
        &gen_dir,
        std::time::SystemTime::now(),
        store::STALE_AFTER,
    );
    built::reap(base, std::time::SystemTime::now(), store::STALE_AFTER);
    if !removed.is_empty() {
        eprintln!(
            "mock: removed {} abandoned lint build(s) under {}",
            removed.len(),
            base.display()
        );
    }
    loaded
}

/// Write the generated cdylib crate (`Cargo.toml` + `src/lib.rs`). Idempotent:
/// rewrites only when content changes, so cargo's freshness check skips a
/// rebuild when nothing moved. `packs` are the specs cargo is to be handed, a
/// branch already replaced by the rev of its tip (`store::resolve_packs`).
pub(crate) fn write_cdylib_crate(
    gen_dir: &Path,
    lints_dir: &Path,
    lint_files: &[String],
    packs: &[(String, String)],
    tools: &[ToolCrate],
    lint_rules_dep: &str,
    cfg: &Config,
) -> Result<(), String> {
    std::fs::create_dir_all(gen_dir.join("src"))
        .map_err(|e| format!("could not create {}: {e}", gen_dir.display()))?;

    let crate_name = gen_crate_name(gen_dir);
    // A leading empty `[workspace]` makes this generated crate its own workspace
    // root. Without it, sitting under `<mock>/target/`, cargo treats it as a
    // member of the consumer's mock workspace and refuses to build it
    // standalone ("current package believes it's in a workspace when it's not").
    let mut manifest = format!(
        "[workspace]\n\n\
         [package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n\
         mockspace = {lint_rules_dep}\n"
    );
    // A pack pinned by branch arrives at the branch's tip, or cargo would hold
    // it at whatever the tip was on the first build. `pack_pin` says why.
    for (name, spec) in packs {
        // A relative path is written as cargo reads it from
        // `<mock>/target/mockspace-lints`, and this crate no longer sits there.
        let spec =
            store::absolutize_path_pack(spec, &crate::build_dir::lint_crate_dir(&cfg.mock_dir));
        manifest.push_str(&format!("{name} = {spec}\n"));
    }
    // Tool crates are path dependencies, discovered by directory rather than
    // declared in `[lint-crates]`. That is the only way they differ from a
    // pack here: a pack is named in config because it comes from elsewhere, and
    // a tool is found because it is in the tree.
    for tool in tools {
        manifest.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            tool.package,
            tool.path.display().to_string().replace('\\', "/")
        ));
    }
    // Force every reference to `mockspace-lint-rules` (the cdylib's own, and any
    // a pack crate pulls in) to the ONE lint-rules the engine is built from. A
    // pack pins lint-rules by `branch = "dev"`; the engine passes it by `rev`;
    // cargo keys those as distinct git sources and builds two copies, whose
    // `Box<dyn Lint>` vtables then differ across the dlopen boundary (E0271 at
    // build, UB if it linked). The `[patch]` collapses them to one. It points at
    // a path (cargo's own checkout of lint-rules at that rev) rather than the git
    // source, because cargo rejects a patch pointing back at the same source.
    let home = cargo_home();
    let mut patch = patch_section(lint_rules_dep, &home);
    if patch.is_none() && is_git_rev_dep(lint_rules_dep) {
        ensure_lint_rules_checkout(gen_dir, lint_rules_dep, &home);
        patch = patch_section(lint_rules_dep, &home);
    }
    if let Some(patch) = patch {
        manifest.push_str(&patch);
    } else if !packs.is_empty() && extract_between(lint_rules_dep, "git = \"", "\"").is_none() {
        // The dep has no git url at all (a path/registry override, e.g. the
        // launcher's `--engine <path>` flag), so there is no revision to patch
        // a pack's own lint-rules reference against. Left alone, a declared
        // `[lint-crates]` pack that pulls in mockspace-lint-rules on its own
        // builds a second, unrelated copy, and the mismatch surfaces as a bare
        // E0271 at cdylib link that names neither this dependency nor the fix.
        return Err(format!(
            "lint-rules dependency `{lint_rules_dep}` has no git url, so it \
             cannot be unified with the engine's own lint-rules revision for \
             the `[lint-crates]` pack(s) this repo declares. A path-declared \
             lint crate cannot be patched to match; declare it with \
             `git = \"...\"` plus a `branch` or `rev` instead."
        ));
    }
    write_if_changed(&gen_dir.join("Cargo.toml"), &manifest)?;

    write_if_changed(
        &gen_dir.join("src").join("lib.rs"),
        &gen_collect_lib(lints_dir, lint_files, packs, tools),
    )?;
    Ok(())
}

/// Build the `[patch]` that pins `mockspace-lint-rules` to the engine's exact
/// lint-rules, so a `[lint-crates]` pack that references lint-rules by branch
/// unifies with the engine's rev-pinned copy instead of building a second one.
///
/// The patch points at a **path**: cargo's own checkout of the engine's source
/// at that rev (`$CARGO_HOME/git/checkouts/<name>-<hash>/<short-rev>/lint-rules`),
/// which cargo just extracted to build the engine. It has to be a path (or any
/// source other than the original git url) because cargo rejects a patch that
/// points back at the same git source. `None` when the dep is not a git-rev spec
/// (a tag/version or a local path override) or the checkout cannot be located,
/// in which case no patch is emitted.
fn patch_section(lint_rules_dep: &str, cargo_home: &Path) -> Option<String> {
    let url = extract_between(lint_rules_dep, "git = \"", "\"")?;
    let rev = extract_between(lint_rules_dep, "rev = \"", "\"")?;
    let path = find_lint_rules_checkout(cargo_home, &url, &rev)?;
    let mut out = String::new();
    for spelling in spellings_of(&url) {
        out.push_str(&format!(
            "\n[patch.\"{spelling}\"]\nmockspace-lint-rules = {{ path = \"{}\" }}\n",
            path.display()
        ));
    }
    Some(out)
}

/// Every url a pack might have written for the same repository.
///
/// Cargo keys a source on the url string, so `https://github.com/o/r.git` and
/// `ssh://git@github.com/o/r.git` are two sources holding two copies of one
/// crate, and a patch on one covers neither the other nor anything that named
/// it. That is the exact failure the patch above exists to prevent, arriving
/// through the one door it did not cover.
///
/// It is not hypothetical and it is not rare: the engine names its own
/// lint-rules by whichever spelling its manifest carries, and a `[lint-crates]`
/// pack in somebody else's repository names it by whichever spelling that
/// author wrote. Both are correct and they do not have to agree.
///
/// So the patch is emitted for both, which costs a table cargo ignores when
/// nothing names that source and is what makes the unification hold when
/// something does.
///
/// Only these two forms, and no attempt to normalise beyond them: a url this
/// does not recognise is passed through unchanged, so a self-hosted forge or a
/// scheme nobody here uses behaves exactly as it did before.
///
/// A `.git`-less spelling is deliberately not a third form. Cargo canonicalises
/// that suffix away, so a patch key differing from the dependency url only by
/// it already matches, while a key naming a different repository does not.
/// Scheme and userinfo are what canonicalisation preserves, which is why this
/// is the axis and the only one.
///
/// **The answers are distinct**, and that is load-bearing rather than tidy: two
/// identical `[patch."X"]` tables is `error: duplicate key`, which cargo raises
/// at manifest parse before resolving anything, so every `cargo mock` in that
/// repository would stop. The dedup is what makes a future third form safe to
/// add without knowing this.
fn spellings_of(url: &str) -> Vec<String> {
    let (https, ssh) = if let Some(rest) = url.strip_prefix("https://") {
        (url.to_string(), format!("ssh://git@{rest}"))
    } else if let Some(rest) = url.strip_prefix("ssh://git@") {
        (format!("https://{rest}"), url.to_string())
    } else {
        return vec![url.to_string()];
    };
    let mut out = vec![https];
    if !out.contains(&ssh) {
        out.push(ssh);
    }
    out
}

/// The throwaway package the fetch runs against: lint-rules and nothing else.
///
/// A workspace of its own, so it does not join the generated crate's, and a lib
/// target, so cargo has a package rather than a manifest it refuses to read.
fn fetch_manifest(lint_rules_dep: &str) -> String {
    format!(
        "[workspace]\n\n\
         [package]\nname = \"lint-rules-fetch\"\nversion = \"0.0.0\"\n\
         edition = \"2024\"\npublish = false\n\n\
         [lib]\npath = \"lib.rs\"\n\n\
         [dependencies]\nmockspace = {lint_rules_dep}\n"
    )
}

/// Whether the dependency names a git repository at a fixed revision, which is
/// the only shape [`patch_section`] can build anything from.
/// The url has to carry a scheme, since cargo refuses a relative one and the
/// fetch would spend a process to be told so.
fn is_git_rev_dep(dep: &str) -> bool {
    extract_between(dep, "git = \"", "\"").is_some_and(|u| u.contains("://"))
        && extract_between(dep, "rev = \"", "\"").is_some()
}

/// Make cargo extract its checkout of `lint-rules` at the engine's revision, so
/// [`patch_section`] has something to point at.
///
/// The patch is built from a path into cargo's checkout, and on a cold cache
/// that checkout does not exist at the moment the manifest is written: cargo
/// extracts it while building this crate, which is afterwards. So the first run
/// in a repository emitted no patch at all, resolved a second copy of
/// `mockspace-lint-rules` for every pack or tool naming it by another spelling,
/// and failed with `E0308` between `LintPack` and `LintPack` at one file and
/// one line, with nothing in the output naming a url. The next run, the
/// checkout now present, worked.
///
/// That is the worst shape a defect takes, because it repairs itself and the
/// repair looks like the thing having been flaky. Observed in `muisti`, where
/// four tools were rewritten to a different url spelling on the strength of it.
///
/// What is fetched is a throwaway package depending on lint-rules and nothing
/// else, rather than the generated crate, since that one is not written yet and
/// its manifest is what this is being computed for. Failure is silent on
/// purpose: the caller asks [`patch_section`] again either way, so a fetch that
/// could not run costs the patch and nothing more, which is where it was.
fn ensure_lint_rules_checkout(gen_dir: &Path, lint_rules_dep: &str, cargo_home: &Path) {
    let dir = gen_dir.join(".fetch");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if std::fs::write(dir.join("lib.rs"), "").is_err() {
        return;
    }
    if write_if_changed(&dir.join("Cargo.toml"), &fetch_manifest(lint_rules_dep)).is_err() {
        return;
    }
    // stderr is inherited: this is where cargo says it is updating a git
    // repository, which is the only thing on screen explaining a cold run's
    // pause. stdout carries nothing worth reading here.
    //
    // `CARGO_HOME` is passed rather than inherited, so the fetch writes its
    // checkout under the same root [`patch_section`] then reads. Inheriting it
    // agrees with that root in every real run and in none that plants one.
    let _ = Command::new("cargo")
        .arg("fetch")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_HOME", cargo_home)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status();
}

/// Locate cargo's checkout of `lint-rules` at `rev` for the git repo `url`,
/// under `<cargo_home>/git/checkouts/<name>-<hash>/<short-rev>/lint-rules`. The
/// `<name>-<hash>` dir is keyed on the url; the sub-dir is the short rev.
fn find_lint_rules_checkout(cargo_home: &Path, url: &str, rev: &str) -> Option<PathBuf> {
    let name = url.rsplit('/').next()?.trim_end_matches(".git");
    let short = &rev[.. rev.len().min(7)];
    let checkouts = cargo_home.join("git").join("checkouts");
    for repo in std::fs::read_dir(&checkouts).ok()?.flatten() {
        if !repo
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("{name}-"))
        {
            continue;
        }
        for co in std::fs::read_dir(repo.path()).ok()?.flatten() {
            if co.file_name().to_string_lossy().starts_with(short) {
                let lr = co.path().join("lint-rules");
                if lr.is_dir() {
                    return Some(lr);
                }
            }
        }
    }
    None
}

/// `$CARGO_HOME` or `~/.cargo`.
fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .unwrap_or_else(|| PathBuf::from(".cargo"))
}

/// The substring of `s` between the first `start` and the next `end` after it.
fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let from = s.find(start)? + start.len();
    let rest = &s[from ..];
    let to = rest.find(end)?;
    Some(rest[.. to].to_string())
}

/// The cdylib `lib.rs`: the repo's in-tree lint files pulled in by path, the
/// pack crates referenced by their (dash-to-underscore) idents, and one
/// `extern "C"` collector that fills the caller's vecs. Mirrors the proxy's
/// `generate_custom_lint_main`, but as a collector rather than a `main`, and
/// against the `mockspace` package-rename so the lint files compile unchanged.
///
/// The `#[path]` of each lint file is the absolute path in the worktree that is
/// running, so this text differs between worktrees and a build after a switch
/// recompiles the generated crate. That is deliberate, and a switch no longer
/// builds at all while the content is one a peer already built: the library is
/// kept under an identity made from its inputs (`built`, `identity`). Making it identical (a symlink inside
/// the store pointing at the running worktree's `lints/`, say) would leave
/// cargo comparing the modification time of whatever the link points at now
/// with the build's record of what it pointed at then, so a switch to a
/// worktree whose lint files are older than the last build would be judged
/// fresh and the other worktree's library loaded. What the worktrees share is
/// the dependency graph, which is the 85 MB; the one small crate is rebuilt.
fn gen_collect_lib(
    lints_dir: &Path,
    lint_files: &[String],
    packs: &[(String, String)],
    tools: &[ToolCrate],
) -> String {
    // generated code: suppress the noise a repo with only lints (no cross, or
    // vice versa) would otherwise emit on the unused collector parameter.
    let mut out = String::from("#![allow(unused)]\n\n");
    for name in lint_files {
        let abs = lints_dir
            .join(format!("{name}.rs"))
            .display()
            .to_string()
            .replace('\\', "/");
        out.push_str(&format!("#[path = \"{abs}\"]\nmod {name};\n"));
    }
    out.push('\n');

    let mut lint_mods = Vec::new();
    let mut cross_mods = Vec::new();
    let mut repo_mods = Vec::new();
    let mut message_mods = Vec::new();
    for name in lint_files {
        let found = scan_lint_functions(lints_dir, name);
        if found.lint {
            lint_mods.push(name.as_str());
        }
        if found.cross_lint {
            cross_mods.push(name.as_str());
        }
        if found.repo_lint {
            repo_mods.push(name.as_str());
        }
        if found.message_lint {
            message_mods.push(name.as_str());
        }
    }
    let pack_idents: Vec<String> = packs.iter().map(|(n, _)| n.replace('-', "_")).collect();

    out.push_str(
        "/// Collect this repo's lints. The pack is owned by the caller (the\n\
         /// engine); the boxes' vtables live in this cdylib, which the engine\n\
         /// keeps loaded for the duration of the lint run. One struct rather\n\
         /// than a vec per kind, so a new lint kind is additive here.\n\
         #[unsafe(no_mangle)]\npub extern \"C\" fn __mockspace_collect_lints(\n\
         \x20   pack: &mut mockspace::LintPack,\n) {\n",
    );
    for m in &lint_mods {
        out.push_str(&format!("    pack.crate_lints.push({m}::lint());\n"));
    }
    for m in &cross_mods {
        out.push_str(&format!(
            "    pack.workspace_lints.push({m}::cross_lint());\n"
        ));
    }
    for m in &repo_mods {
        out.push_str(&format!("    pack.repo_lints.push({m}::repo_lint());\n"));
    }
    for m in &message_mods {
        out.push_str(&format!(
            "    pack.message_lints.push({m}::message_lint());\n"
        ));
    }
    for id in &pack_idents {
        out.push_str(&format!("    {id}::collect(pack);\n"));
    }
    // A tool crate contributes through the same `collect(pack)` entry point a
    // pack uses, generated by the same `lint_pack!` macro. There is no separate
    // tool entry point, because a tool crate may reasonably ship a lint
    // alongside its tool and a second symbol would make that two registrations
    // to keep in step.
    for tool in tools {
        out.push_str(&format!("    {}::collect(pack);\n", tool.ident()));
    }
    out.push_str("}\n");
    out
}

/// The identity of the library built from `content`: a digest of its inputs that names no tree
/// (see `identity`). `None` where they cannot be read.
fn pack_identity(gen_dir: &Path, content: &Content, cfg: &Config) -> Option<identity::Identity> {
    let lints: Vec<(String, PathBuf)> = content
        .lint_files
        .iter()
        .map(|n| (n.clone(), content.lints_dir.join(format!("{n}.rs"))))
        .collect();
    // Generated from the real lint directory, which is what the scan for lint functions reads,
    // and with that directory taken out afterwards so the text names no tree.
    let collector = gen_collect_lib(&content.lints_dir, &content.lint_files, &content.packs, &content.tools)
        .replace(&content.lints_dir.display().to_string().replace('\\', "/"), "<lints>");
    let manifest = gen_dir.join("Cargo.toml");
    let home = cargo_home();
    identity::of_generated_crate(&identity::Tree {
        generated_manifest: &manifest,
        lints_dir:          &content.lints_dir,
        lints:              &lints,
        collector,
        repo_root:          &cfg.repo_root,
        working_dir:        &std::env::current_dir().ok()?,
        cargo_home:         &home,
    })
}

/// Name of the generated crate: the name of the directory it sits in, which is
/// the key `lints_manifest_path` derived from the repo's identity and its revs.
/// Distinct keys are distinct packages, so several of them share one target dir
/// without colliding; the same key from any worktree is the same package.
fn gen_crate_name(gen_dir: &Path) -> String {
    let key: String = gen_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
        .collect();
    format!("mockspace-lints-{key}")
}

/// dlopen the cdylib, call its collector, and return the loaded lints holding
/// the library alive.
///
/// # Safety
///
/// The caller must ensure the cdylib at `dylib` was built against the same
/// `mockspace-lint-rules` pin (and the same toolchain) as this engine, so the
/// `Box<dyn Lint>` vtables the collector hands back match this engine's layout.
/// The launcher enforces this by passing the pin-matched `lint_rules_dep`.
unsafe fn collect(dylib: &Path, identity: Option<String>) -> Result<LoadedLints, String> {
    // SAFETY: loading a cdylib runs its initialisers. This is our own crate,
    // generated and built moments ago from this repo's lints, not arbitrary
    // code of unknown provenance.
    let lib = unsafe { libloading::Library::new(dylib) }
        .map_err(|e| format!("could not load the lint cdylib {}: {e}", dylib.display()))?;
    // `unsafe` on the fn-pointer type is deliberate: calling it is only sound
    // when the cdylib's `mockspace-lint-rules` pin matches ours (the caller
    // invariant), which the type system cannot check. Marking it unsafe forces
    // the `unsafe {}` + SAFETY at the call, reflecting the real contract.
    type Collect = unsafe extern "C" fn(&mut LintPack);
    let mut pack = LintPack::default();
    {
        // the symbol borrows `lib`; scope it so the borrow ends before `lib`
        // moves into the returned struct.
        // SAFETY: `Collect` matches the cdylib's exported signature exactly:
        // both the symbol name and the type are generated together with the
        // collector, so the asserted type cannot disagree with the real one.
        let collect: libloading::Symbol<Collect> =
            unsafe { lib.get(COLLECT_SYMBOL) }.map_err(|e| {
                format!(
                    "the lint cdylib is missing {}: {e}",
                    String::from_utf8_lossy(COLLECT_SYMBOL)
                )
            })?;
        // SAFETY: a call across the C ABI. `LintPack` is `#[repr(Rust)]` but the
        // caller-invariant above guarantees the cdylib's `mockspace-lint-rules`
        // is the same pin as ours, so the struct's layout and every
        // `Box<dyn ...Lint>` vtable inside it agree on both sides (validated:
        // identical trait source, separate compilation, same toolchain).
        unsafe { collect(&mut pack) };
    }
    // Which code these lints are, for the verdicts saved under them. A library that cannot be
    // read back gets an identity no other run shares, so what it saves is never found again
    // rather than found by a different pack with the same empty name.
    pack.identity = identity.or_else(|| crate::lint::verdicts::file_digest(dylib)).unwrap_or_else(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("unreadable-{}-{nanos}", std::process::id())
    });
    Ok(LoadedLints {
        pack,
        _lib: lib,
    })
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(path)
        .map(|c| c == content)
        .unwrap_or(false)
    {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests;
