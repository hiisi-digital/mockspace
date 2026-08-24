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
//! - the repo's own `mock/lints/*.rs` compile into the cdylib, whose build
//!   tree lives in the **project's own `target/`** (they are project-specific
//!   source, not shared machine content);
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

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mockspace_lint_rules::LintPack;

use crate::bootstrap::{
    ToolCrate, discover_custom_lint_files, discover_tool_crates, parse_lint_crates,
    scan_lint_functions,
};
use crate::config::Config;

/// The `extern "C"` symbol the generated cdylib exports.
const COLLECT_SYMBOL: &[u8] = b"__mockspace_collect_lints";

/// Lints loaded from a repo's cdylib, plus the library keeping their vtables
/// alive. Dropping this frees the lints, then unloads the library, in that
/// order (the field order matters: `pack` must drop before `_lib`).
pub struct LoadedLints {
    pub pack: LintPack,
    _lib:     libloading::Library,
}

/// Build and load this repo's custom lints, if it has any. Returns `None` when
/// the repo declares no `mock/lints/*.rs` and no `[lint-crates]` (the common
/// case, and a fast path). `lint_rules_dep` is the cargo dependency *value*
/// for `mockspace-lint-rules`, pin-matched to the running engine, e.g.
/// `{ package = "mockspace-lint-rules", git = "...", rev = "..." }`.
pub fn load(
    cfg: &Config,
    config_path: &Path,
    lint_rules_dep: &str,
) -> Result<Option<LoadedLints>, String> {
    let lints_dir = cfg.mock_dir.join("lints");
    let lint_files = discover_custom_lint_files(&lints_dir);
    let packs = parse_lint_crates(config_path);
    // A repo whose only custom content is a tool still needs the cdylib built,
    // and this early return is what would silently skip it: `mock <tool>` would
    // report an unknown subcommand in a repo that plainly declares one.
    let tools = discover_tool_crates(&cfg.mock_dir.join("tools"));
    if lint_files.is_empty() && packs.is_empty() && tools.is_empty() {
        return Ok(None);
    }

    let gen_dir = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-lints"]);
    write_cdylib_crate(
        &gen_dir,
        &lints_dir,
        &lint_files,
        &packs,
        &tools,
        lint_rules_dep,
        cfg,
    )?;
    let dylib = build_cdylib(&gen_dir, &gen_crate_name(&cfg.mock_dir))?;

    // SAFETY: the cdylib is our own generated crate, built moments ago from
    // this repo's lint sources against the same pinned mockspace-lint-rules
    // the engine uses, so the `Lint`/`CrossCrateLint` vtable layout matches.
    // The returned boxes borrow their vtables from `lib`, so `lib` is stored
    // in `LoadedLints` and outlives them.
    unsafe { collect(&dylib) }.map(Some)
}

/// Write the generated cdylib crate (`Cargo.toml` + `src/lib.rs`). Idempotent:
/// rewrites only when content changes, so cargo's freshness check skips a
/// rebuild when nothing moved.
fn write_cdylib_crate(
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

    let crate_name = gen_crate_name(&cfg.mock_dir);
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
    for (name, spec) in packs {
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
    if let Some(patch) = patch_section(lint_rules_dep, &cargo_home()) {
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
    Some(format!(
        "\n[patch.\"{url}\"]\nmockspace-lint-rules = {{ path = \"{}\" }}\n",
        path.display()
    ))
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
        out.push_str(&format!("    pack.workspace_lints.push({m}::cross_lint());\n"));
    }
    for m in &repo_mods {
        out.push_str(&format!("    pack.repo_lints.push({m}::repo_lint());\n"));
    }
    for m in &message_mods {
        out.push_str(&format!("    pack.message_lints.push({m}::message_lint());\n"));
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

/// Name of the generated crate. Unique per project so several of them can share
/// one target dir without colliding, which is the point of not pinning
/// `--target-dir` below. Run-local hash, not a persisted key.
fn gen_crate_name(mock_dir: &Path) -> String {
    format!("mockspace-lints-{:016x}", path_hash(mock_dir))
}

/// `cargo build` the cdylib and return the built library path.
///
/// **The path comes from cargo, not from a directory listing.** Two reasons,
/// and the second is the one that bit:
///
///   - cargo honours `CARGO_TARGET_DIR`, which is set on any machine sharing
///     one build dir across worktrees, so the artifact need not be under
///     `gen_dir` and a listing there finds nothing;
///   - worse, where a previous run left a file in the expected place, that
///     stale copy loads and the engine answers from an old build of the
///     project's lints and tools, silently. Reads exactly like an edit not
///     taking effect, and cost two rounds chasing a phantom loader bug.
///
/// Pinning `--target-dir` fixes both and forecloses the cache: every fresh
/// clone, worktree and fixture then pays a cold release build of the whole dep
/// graph, which is what saturated a laptop. Taking the path out of
/// `--message-format json` fixes both and keeps the cache, because the path
/// comes from the run that produced it and cannot be a leftover.
fn build_cdylib(gen_dir: &Path, crate_name: &str) -> Result<PathBuf, String> {
    // json-render-diagnostics puts artifacts on stdout and still renders
    // warnings and errors to stderr the way a human expects.
    let mut child = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(gen_dir.join("Cargo.toml"))
        .arg("--message-format")
        .arg("json-render-diagnostics")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not run cargo build for the lint cdylib: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cargo build gave no stdout to read artifacts from".to_string())?;
    let mut found = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(p) = cdylib_artifact(&line, crate_name) {
            found = Some(p);
        }
    }

    // NOTE: stdout drained first, else a full pipe deadlocks the wait
    let status = child
        .wait()
        .map_err(|e| format!("could not wait on cargo build for the lint cdylib: {e}"))?;
    if !status.success() {
        return Err("building the custom-lint cdylib failed; see cargo output above".to_string());
    }

    let dylib = found.ok_or_else(|| {
        format!(
            "cargo reported success but emitted no cdylib artifact for \
             `{crate_name}`, so the build produced nothing this engine can load"
        )
    })?;
    // NOTE: the path is known now, so the tree cargo actually wrote is known
    // too, and that is the one worth keeping out of the desktop index. Under
    // CARGO_TARGET_DIR it is nowhere this engine computed, so marking
    // `<mock>/target` marks an almost-empty directory and leaves the large one
    // indexed. Only reachable here because the path came from cargo.
    crate::build_dir::mark_target_root_of(&dylib);
    Ok(dylib)
}

/// One line of cargo's json, to the cdylib path it names, or nothing.
///
/// NOTE: cargo normalises a lib target's name, so the package
/// `mockspace-lints-<hash>` arrives here as `mockspace_lints_<hash>`. matching
/// on the package name finds nothing at all, silently, which is why the
/// substitution is done rather than assumed.
fn cdylib_artifact(line: &str, crate_name: &str) -> Option<PathBuf> {
    let msg: serde_json::Value = serde_json::from_str(line).ok()?;
    if msg.get("reason")? != "compiler-artifact" {
        return None;
    }
    let target = msg.get("target")?;
    if target.get("name")? != crate_name.replace('-', "_").as_str() {
        return None;
    }
    if !target
        .get("kind")?
        .as_array()?
        .iter()
        .any(|k| k == "cdylib")
    {
        return None;
    }
    // one target can emit several files; take the loadable one
    msg.get("filenames")?
        .as_array()?
        .iter()
        .filter_map(|f| f.as_str())
        .map(PathBuf::from)
        .find(|p| {
            p.extension()
                .is_some_and(|e| e == "dylib" || e == "so" || e == "dll")
        })
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
unsafe fn collect(dylib: &Path) -> Result<LoadedLints, String> {
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
    Ok(LoadedLints {
        pack,
        _lib: lib,
    })
}

fn path_hash(p: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
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
mod tests {
    use super::*;

    // one real `compiler-artifact` line, out of a live
    // `cargo build --message-format json-render-diagnostics`. hand-written json
    // here would only test that the test agrees with itself.
    const ARTIFACT: &str = r#"{"reason":"compiler-artifact","package_id":"path+file:///p/mock/target/mockspace-lints#mockspace-lints-40d923802ebb4e36@0.0.0","manifest_path":"/p/mock/target/mockspace-lints/Cargo.toml","target":{"kind":["cdylib"],"crate_types":["cdylib"],"name":"mockspace_lints_40d923802ebb4e36","src_path":"/p/mock/target/mockspace-lints/src/lib.rs","edition":"2024","doc":false,"doctest":false,"test":true},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":false},"features":[],"filenames":["/elsewhere/release/libmockspace_lints_40d923802ebb4e36.dylib"],"executable":null,"fresh":true}"#;

    #[test]
    fn the_artifact_path_comes_from_cargo_not_from_a_directory() {
        // the whole mechanism: the path is wherever cargo put it, which under
        // an inherited CARGO_TARGET_DIR is not under gen_dir at all.
        assert_eq!(
            cdylib_artifact(ARTIFACT, "mockspace-lints-40d923802ebb4e36"),
            Some(PathBuf::from(
                "/elsewhere/release/libmockspace_lints_40d923802ebb4e36.dylib"
            ))
        );
    }

    #[test]
    fn a_lib_target_name_is_normalised_so_the_package_name_never_matches() {
        // cargo turns `mockspace-lints-<hash>` into `mockspace_lints_<hash>`.
        // matching the package name finds nothing, silently, and the engine
        // then reports a successful build with no artifact. drop the
        // substitution and this arm is what goes red.
        assert!(ARTIFACT.contains(r#""name":"mockspace_lints_40d923802ebb4e36""#));
        assert!(cdylib_artifact(ARTIFACT, "mockspace-lints-40d923802ebb4e36").is_some());
    }

    #[test]
    fn another_projects_artifact_is_not_ours() {
        // several generated crates can share one target dir, which is why the
        // name is hashed per project in the first place.
        assert!(cdylib_artifact(ARTIFACT, "mockspace-lints-0000000000000000").is_none());
    }

    #[test]
    fn nothing_but_a_cdylib_artifact_answers() {
        // controls. without these, a filter that always took the first
        // filename would pass every arm above.
        let lib = ARTIFACT.replace(r#""kind":["cdylib"]"#, r#""kind":["lib"]"#);
        assert!(cdylib_artifact(&lib, "mockspace-lints-40d923802ebb4e36").is_none());
        let other = ARTIFACT.replace("compiler-artifact", "build-script-executed");
        assert!(cdylib_artifact(&other, "mockspace-lints-40d923802ebb4e36").is_none());
        assert!(cdylib_artifact("not json at all", "mockspace-lints-40d923802ebb4e36").is_none());
        let rlib = ARTIFACT.replace(".dylib", ".rlib");
        assert!(cdylib_artifact(&rlib, "mockspace-lints-40d923802ebb4e36").is_none());
    }

    #[test]
    fn collector_lib_shape() {
        let dir = tempfile::tempdir().unwrap();
        let lints = dir.path().join("lints");
        std::fs::create_dir_all(&lints).unwrap();
        std::fs::write(
            lints.join("foo.rs"),
            "pub fn lint() -> Box<dyn mockspace::Lint> { todo!() }\n",
        )
        .unwrap();
        let src = gen_collect_lib(
            &lints,
            &["foo".to_string()],
            &[("some-pack".into(), "\"1\"".into())],
            &[],
        );
        assert!(src.contains("#[unsafe(no_mangle)]"));
        assert!(src.contains("pub extern \"C\" fn __mockspace_collect_lints"));
        assert!(src.contains("mod foo;"));
        // one struct across the boundary, so a new lint kind is additive
        assert!(src.contains("pack: &mut mockspace::LintPack"));
        assert!(src.contains("pack.crate_lints.push(foo::lint());"));
        // pack idents dash->underscore, and one collect() call per pack rather
        // than one call per lint kind
        assert!(src.contains("some_pack::collect(pack);"));
        // foo has no cross_lint(), so no per-file workspace push for it
        assert!(!src.contains("pack.workspace_lints.push(foo::cross_lint());"));
    }

    /// All four entry points, which the scanner recognised as two until
    /// recently. `RepoLint` is the one handed paths rather than packages, so it
    /// is the only kind a repository with no packages can register, and it was
    /// reachable from an imported pack and from nowhere else.
    ///
    /// The trap is in the names. `cross_lint(`, `repo_lint(` and
    /// `message_lint(` all end in `lint(`, so a probe for the plain kind that
    /// is not anchored on the space after `fn` claims every file that defines
    /// any of them. That is what the negatives below are for, and the scanner
    /// carries a comment about it while pinning nothing.
    #[test]
    fn every_entry_point_is_recognised_and_none_stands_in_for_another() {
        let dir = tempfile::tempdir().unwrap();
        let lints = dir.path().join("lints");
        std::fs::create_dir_all(&lints).unwrap();

        for (stem, decl) in [
            ("plain", "pub fn lint()"),
            ("cross", "pub fn cross_lint()"),
            ("repo", "pub fn repo_lint()"),
            ("msg", "pub fn message_lint()"),
        ] {
            std::fs::write(lints.join(format!("{stem}.rs")), format!("{decl} {{ todo!() }}\n"))
                .unwrap();
        }

        let modules: Vec<String> =
            ["plain", "cross", "repo", "msg"].iter().map(|s| s.to_string()).collect();
        let src = gen_collect_lib(&lints, &modules, &[], &[]);

        assert!(src.contains("pack.crate_lints.push(plain::lint());"));
        assert!(src.contains("pack.workspace_lints.push(cross::cross_lint());"));
        assert!(src.contains("pack.repo_lints.push(repo::repo_lint());"));
        assert!(src.contains("pack.message_lints.push(msg::message_lint());"));

        // the part with teeth: a file defining only one kind registers only
        // that kind, so a suffix match on `lint(` cannot pass this
        assert!(!src.contains("pack.crate_lints.push(cross::lint());"));
        assert!(!src.contains("pack.crate_lints.push(repo::lint());"));
        assert!(!src.contains("pack.crate_lints.push(msg::lint());"));
        assert!(!src.contains("pack.repo_lints.push(plain::repo_lint());"));
        assert!(!src.contains("pack.message_lints.push(plain::message_lint());"));
    }

    /// A file may define more than one, and each is pushed once onto its own
    /// list. The control for the test above, which uses one kind per file and
    /// so cannot tell "recognises each" from "recognises whichever it sees
    /// first".
    #[test]
    fn one_file_may_register_several_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let lints = dir.path().join("lints");
        std::fs::create_dir_all(&lints).unwrap();
        std::fs::write(
            lints.join("both.rs"),
            "pub fn lint() { todo!() }\npub fn repo_lint() { todo!() }\n",
        )
        .unwrap();

        let src = gen_collect_lib(&lints, &["both".to_string()], &[], &[]);
        assert!(src.contains("pack.crate_lints.push(both::lint());"));
        assert!(src.contains("pack.repo_lints.push(both::repo_lint());"));
        assert!(!src.contains("pack.workspace_lints.push(both::cross_lint());"));
        assert!(!src.contains("pack.message_lints.push(both::message_lint());"));
    }

    fn tool(dir: &str, package: &str, at: &Path) -> ToolCrate {
        ToolCrate {
            dir:     dir.to_string(),
            package: package.to_string(),
            path:    at.join(dir),
        }
    }

    #[test]
    fn a_tool_crate_is_collected_through_the_same_entry_point_a_pack_uses() {
        let dir = tempfile::tempdir().unwrap();
        let lints = dir.path().join("lints");
        std::fs::create_dir_all(&lints).unwrap();
        let t = tool("phrase-search", "kamu-phrase-search", dir.path());

        let src = gen_collect_lib(&lints, &[], &[], std::slice::from_ref(&t));
        // dash-to-underscore on the package name, and one collect() call, so a
        // tool crate shipping a lint beside its tool registers both.
        assert!(src.contains("kamu_phrase_search::collect(pack);"), "{src}");
        // and it must NOT invent a tool-specific symbol: the negative that
        // pins "same pattern as lints" rather than a parallel path.
        assert!(!src.contains("kamu_phrase_search::tool()"), "{src}");
        assert!(!src.contains("pack.tools.push"), "{src}");
    }

    #[test]
    fn a_tool_crate_lands_on_the_dependency_line_by_package_name_and_path() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config::from_dir(dir.path());
        let gen_dir = dir.path().join("gen_dir");
        let t = tool("phrase-search", "kamu-phrase-search", dir.path());
        write_cdylib_crate(
            &gen_dir,
            &dir.path().join("lints"),
            &[],
            &[],
            std::slice::from_ref(&t),
            "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }",
            &cfg,
        )
        .unwrap();
        let manifest = std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains(&format!(
                "kamu-phrase-search = {{ path = \"{}\" }}",
                t.path.display()
            )),
            "{manifest}"
        );
        // The case that must fail: depending on the DIRECTORY name would emit
        // `phrase-search = ...`, which cargo cannot resolve to that package.
        assert!(
            !manifest.contains("\nphrase-search = {"),
            "the directory name must not be used as the package name: {manifest}"
        );
    }

    #[test]
    fn a_repo_with_only_tools_still_gets_a_cdylib() {
        // The early return in `load` short-circuits on "no lints and no packs".
        // Before tools were counted there, a repo whose only custom content is
        // a tool built nothing and `mock <tool>` found nothing.
        let dir = tempfile::tempdir().unwrap();
        let mock = dir.path().join("mock");
        std::fs::create_dir_all(mock.join("tools").join("only-tool").join("src")).unwrap();
        std::fs::write(
            mock.join("tools").join("only-tool").join("Cargo.toml"),
            "[package]\nname = \"only-tool\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        std::fs::write(mock.join("mockspace.toml"), "project_name = \"x\"\n").unwrap();

        // no mock/lints, no [lint-crates]: the two things the old guard checked
        assert!(!mock.join("lints").exists());
        assert!(parse_lint_crates(&mock.join("mockspace.toml")).is_empty());
        // and yet there is something to build
        assert_eq!(discover_tool_crates(&mock.join("tools")).len(), 1);
    }

    #[test]
    fn manifest_renames_lint_rules_to_mockspace() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config::from_dir(dir.path());
        let gen_dir = dir.path().join("gen_dir");
        let dep = "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }";
        write_cdylib_crate(
            &gen_dir,
            &dir.path().join("lints"),
            &[],
            &[("p".into(), "\"1\"".into())],
            &[],
            dep,
            &cfg,
        )
        .unwrap();
        let manifest = std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("crate-type = [\"cdylib\"]"));
        assert!(manifest.contains(&format!("mockspace = {dep}")));
        assert!(manifest.contains("p = \"1\""));
        // a leading [workspace] makes the crate its own root so cargo builds it
        // standalone under the consumer's <mock>/target/ (not as a member).
        assert!(manifest.trim_start().starts_with("[workspace]"));
    }

    #[test]
    fn patch_section_points_at_the_checkout_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        // lay out cargo's checkout: git/checkouts/<name>-<hash>/<short-rev>/lint-rules
        let lr = home.join("git/checkouts/mockspace-deadbeef/abc1234/lint-rules");
        std::fs::create_dir_all(&lr).unwrap();
        let dep = "{ package = \"mockspace-lint-rules\", git = \"ssh://x/mockspace.git\", rev = \"abc1234ff\" }";
        let patch = patch_section(dep, home).unwrap();
        assert!(patch.contains("[patch.\"ssh://x/mockspace.git\"]"));
        assert!(patch.contains(&format!(
            "mockspace-lint-rules = {{ path = \"{}\" }}",
            lr.display()
        )));
    }

    #[test]
    fn patch_section_none_when_checkout_absent_or_not_git() {
        let tmp = tempfile::tempdir().unwrap();
        // git-rev dep but no checkout on disk -> no patch.
        let dep = "{ package = \"mockspace-lint-rules\", git = \"ssh://x/m.git\", rev = \"abc\" }";
        assert!(patch_section(dep, tmp.path()).is_none());
        // path/registry override -> no patch.
        assert!(patch_section("{ path = \"../lint-rules\" }", tmp.path()).is_none());
    }
}
