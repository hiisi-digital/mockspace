//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

#![allow(unused_imports)]

use super::*;

/// Check if every crate in the workspace has been nuked.
pub(crate) fn detect_nuked_workspace(cfg: &Config) -> bool {
    // Every source directory. Answering this from one group would report a
    // fully nuked workspace while other groups still held their source, and
    // this is the check other behaviour keys off.
    let crate_dirs = crate::parse::package_dirs_in(&cfg.src_dirs);

    if crate_dirs.is_empty() {
        return false;
    }

    crate_dirs.iter().all(|dir| {
        let librs = dir.join("src/lib.rs");
        fs::read_to_string(&librs)
            .map(|s| s.contains(&cfg.nuke_marker))
            .unwrap_or(false)
    })
}

/// Wipe all mock crate source code, leaving minimal lib.rs stubs.
pub(crate) fn nuke_mock_sources(cfg: &Config) -> ExitCode {
    eprintln!("--- NUKE: wiping all mock crate source ---");
    eprintln!("    design docs and Cargo.toml files are preserved");
    eprintln!();

    let mut nuked_files = 0u32;
    let mut nuked_crates = 0u32;

    // Every source directory. A nuke that covered one group and reported
    // success would leave the rest of the workspace holding source that every
    // later step assumes is gone.
    let entries = crate::parse::package_dirs_in(&cfg.src_dirs);

    for path in entries {
        let crate_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }

        let cargo_toml = path.join("Cargo.toml");
        let is_proc_macro = fs::read_to_string(&cargo_toml)
            .map(|c| c.contains("proc-macro = true"))
            .unwrap_or(false);

        let deleted = delete_non_lib_rs(&src_dir);
        nuked_files += deleted;

        let lib_rs = src_dir.join("lib.rs");
        let stub = if is_proc_macro {
            format!(
                "//! {crate_name}: proc macro crate.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n\
                 \n\
                 extern crate proc_macro;\n",
                cfg.nuke_marker
            )
        } else {
            format!(
                "//! {crate_name}: nuked.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n",
                cfg.nuke_marker
            )
        };

        if lib_rs.exists() {
            nuked_files += 1;
        }
        fs::write(&lib_rs, &stub).expect("failed to write lib.rs stub");
        nuked_crates += 1;
        eprintln!("  nuked: {crate_name}");
    }

    eprintln!();
    eprintln!("--- NUKE complete: {nuked_files} files across {nuked_crates} crates ---");
    eprintln!("    cargo check will fail until source is rewritten from docs");
    ExitCode::SUCCESS
}

pub(crate) fn delete_non_lib_rs(dir: &Path) -> u32 {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += delete_all_rs(&path);
                let _ = fs::remove_dir(&path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let name = path.file_name().unwrap().to_string_lossy();
                if name != "lib.rs" {
                    let _ = fs::remove_file(&path);
                    count += 1;
                }
            }
        }
    }
    count
}

pub(crate) fn delete_all_rs(dir: &Path) -> u32 {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += delete_all_rs(&path);
                let _ = fs::remove_dir(&path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let _ = fs::remove_file(&path);
                count += 1;
            }
        }
    }
    count
}

