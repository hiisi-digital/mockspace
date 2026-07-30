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

use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_lint_rules::LintPack;

use crate::bootstrap::{discover_custom_lint_files, parse_lint_crates, scan_lint_functions};
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
    if lint_files.is_empty() && packs.is_empty() {
        return Ok(None);
    }

    let gen_dir = cfg.mock_dir.join("target").join("mockspace-lints");
    write_cdylib_crate(
        &gen_dir,
        &lints_dir,
        &lint_files,
        &packs,
        lint_rules_dep,
        cfg,
    )?;
    let dylib = build_cdylib(&gen_dir)?;

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
    lint_rules_dep: &str,
    cfg: &Config,
) -> Result<(), String> {
    std::fs::create_dir_all(gen_dir.join("src"))
        .map_err(|e| format!("could not create {}: {e}", gen_dir.display()))?;

    // A crate name unique to this project so builds in a shared target dir
    // (a future optimisation) never collide. A run-local hash suffices; this
    // is not a persisted key.
    let crate_name = format!("mockspace-lints-{:016x}", path_hash(&cfg.mock_dir));
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
        &gen_collect_lib(lints_dir, lint_files, packs),
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
fn gen_collect_lib(lints_dir: &Path, lint_files: &[String], packs: &[(String, String)]) -> String {
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
    for name in lint_files {
        let (has_lint, has_cross) = scan_lint_functions(lints_dir, name);
        if has_lint {
            lint_mods.push(name.as_str());
        }
        if has_cross {
            cross_mods.push(name.as_str());
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
    for id in &pack_idents {
        out.push_str(&format!("    {id}::collect(pack);\n"));
    }
    out.push_str("}\n");
    out
}

/// `cargo build` the cdylib and return the built library path.
fn build_cdylib(gen_dir: &Path) -> Result<PathBuf, String> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(gen_dir.join("Cargo.toml"))
        .status()
        .map_err(|e| format!("could not run cargo build for the lint cdylib: {e}"))?;
    if !status.success() {
        return Err("building the custom-lint cdylib failed; see cargo output above".to_string());
    }
    // cdylib lands in <gen_dir>/target/release/ with the platform library prefix
    // and extension.
    let rel = gen_dir.join("target").join("release");
    for (prefix, ext) in [("lib", "dylib"), ("lib", "so"), ("", "dll")] {
        if let Ok(rd) = std::fs::read_dir(&rel) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with(&format!("{prefix}mockspace_lints_"))
                    && p.extension().map(|x| x == ext).unwrap_or(false)
                {
                    return Ok(p);
                }
            }
        }
    }
    Err(format!(
        "cargo reported success but no lint cdylib was found in {}",
        rel.display()
    ))
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
        let src = gen_collect_lib(&lints, &["foo".to_string()], &[(
            "some-pack".into(),
            "\"1\"".into(),
        )]);
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
