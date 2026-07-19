#![allow(unused_imports)]
use super::*;

/// Check if every crate in the workspace has been nuked.
pub(crate) fn detect_nuked_workspace(cfg: &Config) -> bool {
    let entries = match fs::read_dir(&cfg.crates_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    let crate_dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();

    if crate_dirs.is_empty() {
        return false;
    }

    crate_dirs.iter().all(|entry| {
        let librs = entry.path().join("src/lib.rs");
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

    let mut entries: Vec<_> = fs::read_dir(&cfg.crates_dir)
        .expect("can't read crates dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let src_dir = entry.path().join("src");
        if !src_dir.exists() {
            continue;
        }

        let cargo_toml = entry.path().join("Cargo.toml");
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

/// Resolve a `--dir` argument to an absolute path containing `mockspace.toml`.
///
/// Tries in order:
/// 1. Absolute path as-is (already absolute from bootstrap alias)
/// 2. Relative path from CWD (user running from repo root)
/// 3. Relative path from git repo root (user running from a subdirectory
///    with a stale relative-path alias)
///
/// Falls back to the raw path if nothing matches, so downstream code
/// can produce a clear "no mockspace.toml found" error.
/// djb2 hash for detecting proxy Cargo.toml changes across cargo check runs.
pub(crate) fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h
}
