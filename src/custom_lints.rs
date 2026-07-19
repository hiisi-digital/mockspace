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
//! both sides resolve `mockspace-lint-rules` at the *same* pin and build with
//! the same toolchain, so the trait's vtable layout is identical. The launcher
//! enforces that by passing the pin-matched dep spec (`lint_rules_dep`), the
//! same version it built the engine from. Validated experimentally (identical
//! trait source compiled separately still dispatches correctly).

use std::path::{Path, PathBuf};
use std::process::Command;

use mockspace_lint_rules::{CrossCrateLint, Lint};

use crate::bootstrap::{discover_custom_lint_files, parse_lint_crates, scan_lint_functions};
use crate::config::Config;

/// The `extern "C"` symbol the generated cdylib exports.
const COLLECT_SYMBOL: &[u8] = b"__mockspace_collect_lints";

/// Lints loaded from a repo's cdylib, plus the library keeping their vtables
/// alive. Dropping this frees the lints, then unloads the library, in that
/// order (the field order matters: `lints`/`cross` must drop before `_lib`).
pub struct LoadedLints {
    pub lints: Vec<Box<dyn Lint>>,
    pub cross: Vec<Box<dyn CrossCrateLint>>,
    _lib: libloading::Library,
}

/// Build and load this repo's custom lints, if it has any. Returns `None` when
/// the repo declares no `mock/lints/*.rs` and no `[lint-crates]` (the common
/// case, and a fast path). `lint_rules_dep` is the cargo dependency *value*
/// for `mockspace-lint-rules`, pin-matched to the running engine, e.g.
/// `{ package = "mockspace-lint-rules", git = "...", rev = "..." }`.
pub fn load(cfg: &Config, config_path: &Path, lint_rules_dep: &str) -> Result<Option<LoadedLints>, String> {
    let lints_dir = cfg.mock_dir.join("lints");
    let lint_files = discover_custom_lint_files(&lints_dir);
    let packs = parse_lint_crates(config_path);
    if lint_files.is_empty() && packs.is_empty() {
        return Ok(None);
    }

    let gen_dir = cfg.mock_dir.join("target").join("mockspace-lints");
    write_cdylib_crate(&gen_dir, &lints_dir, &lint_files, &packs, lint_rules_dep, cfg)?;
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
    let mut manifest = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n\
         mockspace = {lint_rules_dep}\n"
    );
    for (name, spec) in packs {
        manifest.push_str(&format!("{name} = {spec}\n"));
    }
    write_if_changed(&gen_dir.join("Cargo.toml"), &manifest)?;

    write_if_changed(&gen_dir.join("src").join("lib.rs"), &gen_collect_lib(lints_dir, lint_files, packs))?;
    Ok(())
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
        let abs = lints_dir.join(format!("{name}.rs")).display().to_string().replace('\\', "/");
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
        "/// Collect this repo's lints. Both vecs are owned by the caller (the\n\
         /// engine); the boxes' vtables live in this cdylib, which the engine\n\
         /// keeps loaded for the duration of the lint run.\n\
         #[unsafe(no_mangle)]\npub extern \"C\" fn __mockspace_collect_lints(\n\
         \x20   lints: &mut Vec<Box<dyn mockspace::Lint>>,\n\
         \x20   cross: &mut Vec<Box<dyn mockspace::CrossCrateLint>>,\n) {\n",
    );
    for m in &lint_mods {
        out.push_str(&format!("    lints.push({m}::lint());\n"));
    }
    for id in &pack_idents {
        out.push_str(&format!("    lints.extend({id}::lints());\n"));
    }
    for m in &cross_mods {
        out.push_str(&format!("    cross.push({m}::cross_lint());\n"));
    }
    for id in &pack_idents {
        out.push_str(&format!("    cross.extend({id}::cross_lints());\n"));
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
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name.starts_with(&format!("{prefix}mockspace_lints_"))
                    && p.extension().map(|x| x == ext).unwrap_or(false)
                {
                    return Ok(p);
                }
            }
        }
    }
    Err(format!("cargo reported success but no lint cdylib was found in {}", rel.display()))
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
    type Collect = unsafe extern "C" fn(&mut Vec<Box<dyn Lint>>, &mut Vec<Box<dyn CrossCrateLint>>);
    let mut lints: Vec<Box<dyn Lint>> = Vec::new();
    let mut cross: Vec<Box<dyn CrossCrateLint>> = Vec::new();
    {
        // the symbol borrows `lib`; scope it so the borrow ends before `lib`
        // moves into the returned struct.
        // SAFETY: `Collect` matches the cdylib's exported signature exactly:
        // both the symbol name and the type are generated together with the
        // collector, so the asserted type cannot disagree with the real one.
        let collect: libloading::Symbol<Collect> = unsafe { lib.get(COLLECT_SYMBOL) }.map_err(|e| {
            format!("the lint cdylib is missing {}: {e}", String::from_utf8_lossy(COLLECT_SYMBOL))
        })?;
        // SAFETY: a call across the C ABI. The vecs are `#[repr(Rust)]` but the
        // caller-invariant above guarantees the cdylib's `mockspace-lint-rules`
        // is the same pin as ours, so `Box<dyn Lint>`'s layout agrees on both
        // sides (validated: identical trait source, separate compilation, same
        // toolchain -> identical vtable layout).
        unsafe { collect(&mut lints, &mut cross) };
    }
    Ok(LoadedLints { lints, cross, _lib: lib })
}

fn path_hash(p: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
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
        std::fs::write(lints.join("foo.rs"), "pub fn lint() -> Box<dyn mockspace::Lint> { todo!() }\n").unwrap();
        let src = gen_collect_lib(&lints, &["foo".to_string()], &[("some-pack".into(), "\"1\"".into())]);
        assert!(src.contains("#[unsafe(no_mangle)]"));
        assert!(src.contains("pub extern \"C\" fn __mockspace_collect_lints"));
        assert!(src.contains("mod foo;"));
        assert!(src.contains("lints.push(foo::lint());"));
        // pack idents dash->underscore, both lints() and cross_lints()
        assert!(src.contains("lints.extend(some_pack::lints());"));
        assert!(src.contains("cross.extend(some_pack::cross_lints());"));
        // foo has no cross_lint(), so no per-file cross push for it
        assert!(!src.contains("cross.push(foo::cross_lint());"));
    }

    #[test]
    fn manifest_renames_lint_rules_to_mockspace() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config::from_dir(dir.path());
        let gen_dir = dir.path().join("gen_dir");
        let dep = "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }";
        write_cdylib_crate(&gen_dir, &dir.path().join("lints"), &[], &[("p".into(), "\"1\"".into())], dep, &cfg).unwrap();
        let manifest = std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("crate-type = [\"cdylib\"]"));
        assert!(manifest.contains(&format!("mockspace = {dep}")));
        assert!(manifest.contains("p = \"1\""));
    }
}
