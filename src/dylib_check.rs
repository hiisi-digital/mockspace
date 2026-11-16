//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Dylib module loading check.
//!
//! Builds the workspace, then dlopen's each dylib module crate and verifies:
//! 1. The ABI version function returns the expected version
//! 2. The manifest function is callable
//! 3. The init function exists
//! 4. The shutdown function exists and is callable

use std::path::Path;

use crate::config::Config;

/// Check all module dylibs. Returns the number of failures.
pub fn check_module_dylibs(cfg: &Config) -> usize {
    let mut failures = 0;
    let sym_prefix = cfg.crate_prefix.replace('-', "_");

    for crate_name in &cfg.module_crates {
        let dylib_path = module_library(&cfg.mock_dir, crate_name);

        if !dylib_path.exists() {
            eprintln!(
                "  FAIL {crate_name}: dylib not found at {}",
                dylib_path.display()
            );
            failures += 1;
            continue;
        }

        match check_one_dylib(&dylib_path, cfg.abi_version, &sym_prefix) {
            Ok(()) => eprintln!("  ok {crate_name}"),
            Err(e) => {
                eprintln!("  FAIL {crate_name}: {e}");
                failures += 1;
            },
        }
    }

    failures
}

/// Where `cargo build` puts the module crate `crate_name` of the workspace at
/// `mock_dir`: the debug profile, named as this host names a dynamic library,
/// since the check loads it on this host (`libx.so`, `libx.dylib`, `x.dll`).
fn module_library(mock_dir: &Path, crate_name: &str) -> std::path::PathBuf {
    mock_dir.join("target/debug").join(format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        crate_name.replace('-', "_"),
        std::env::consts::DLL_SUFFIX
    ))
}

fn check_one_dylib(path: &Path, expected_abi: u32, sym_prefix: &str) -> Result<(), String> {
    let abi_sym = format!("__{sym_prefix}_abi_version");
    let manifest_sym = format!("__{sym_prefix}_manifest");
    let init_sym = format!("__{sym_prefix}_init");
    let shutdown_sym = format!("__{sym_prefix}_shutdown");

    // Safety: we only read static data and call trivial functions.
    // The dylibs are our own code, built seconds ago.
    unsafe {
        let lib = libloading::Library::new(path).map_err(|e| format!("dlopen failed: {e}"))?;

        // 1. ABI version handshake
        let abi_fn: libloading::Symbol<extern "C" fn() -> u32> = lib
            .get(abi_sym.as_bytes())
            .map_err(|e| format!("missing {abi_sym}: {e}"))?;

        let ver = abi_fn();
        if ver != expected_abi {
            return Err(format!(
                "ABI version mismatch: got {ver}, expected {expected_abi}"
            ));
        }

        // 2. Manifest function exists
        let _manifest_fn: libloading::Symbol<extern "C" fn()> = lib
            .get(manifest_sym.as_bytes())
            .map_err(|e| format!("missing {manifest_sym}: {e}"))?;

        // 3. Init function exists
        let _init_fn: libloading::Symbol<extern "C" fn()> = lib
            .get(init_sym.as_bytes())
            .map_err(|e| format!("missing {init_sym}: {e}"))?;

        // 4. Shutdown function exists and is safe to call (default is no-op)
        let shutdown_fn: libloading::Symbol<extern "C" fn()> = lib
            .get(shutdown_sym.as_bytes())
            .map_err(|e| format!("missing {shutdown_sym}: {e}"))?;

        shutdown_fn();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::module_library;

    /// The library is looked for under the name this host gives it; a fixed
    /// `lib<x>.dylib` found nothing on Linux or Windows.
    #[test]
    fn a_module_library_is_named_as_this_host_names_one() {
        let path = module_library(Path::new("/m"), "my-module");
        let expected = if cfg!(target_os = "macos") {
            "/m/target/debug/libmy_module.dylib"
        } else if cfg!(windows) {
            "/m/target/debug/my_module.dll"
        } else {
            "/m/target/debug/libmy_module.so"
        };
        assert_eq!(path, Path::new(expected));
    }
}
