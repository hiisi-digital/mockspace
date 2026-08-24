//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The per-version build cache.
//!
//! Each distinct compilation input (mockspace url + rev, plus repo-specific
//! lint inputs) is built once into `~/.cache/mockspace/builds/<key>/bin/
//! mockspace` and shared by every repo pinned to it. `cargo install` takes
//! its own lock on the install root and installs the binary atomically, so a
//! racing second launcher either blocks on that lock or finds the finished
//! binary; the launcher needs no lock of its own.
//!
//! The cache dir lives under `~/.cache` (honoring `XDG_CACHE_HOME`), the
//! machine-content-cache slot in v2's taxonomy, never under `~/.config`
//! (reserved for per-developer config).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::hash::Fnv;
use crate::pin::Resolved;

/// `$XDG_CACHE_HOME/mockspace` or `~/.cache/mockspace`.
pub fn cache_root() -> Result<PathBuf, String> {
    cache_root_from(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"))
}

/// Pure core of [`cache_root`]: env values passed in so it is testable without
/// mutating process env (cargo runs tests in parallel threads, where `set_var`
/// is a data race).
fn cache_root_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    if let Some(x) = xdg
        && !x.is_empty()
    {
        return Ok(PathBuf::from(x).join("mockspace"));
    }
    let home = home
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "neither XDG_CACHE_HOME nor HOME is set".to_string())?;
    Ok(PathBuf::from(home).join(".cache").join("mockspace"))
}

fn builds_dir(root: &Path) -> PathBuf {
    root.join("builds")
}

/// The toolchain identity to fold into the cache key: `rustc -vV` (version,
/// commit hash, host, LLVM). rustc is part of the real compilation input, so a
/// toolchain change must re-key the cached engine, or a frozen engine binary
/// would be paired with a freshly-built lint cdylib compiled by a different
/// rustc, whose `Box<dyn Lint>` vtable layout may differ (UB across the dlopen
/// boundary). Empty string when rustc cannot be run (the key then omits it; the
/// build itself would fail downstream anyway).
pub fn rustc_fingerprint() -> String {
    Command::new("rustc")
        .arg("-vV")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The cache key: a hash of the full compilation input. The mockspace url and
/// resolved rev, the `toolchain` identity (see [`rustc_fingerprint`]), and
/// `lint_inputs` (repo-specific lint sources, so a repo with custom lints keys
/// its own binary). A change in any re-keys and forces a coherent rebuild.
pub fn compute_key(
    url: &str,
    key_rev: &str,
    toolchain: &str,
    lint_inputs: &[(String, Vec<u8>)],
) -> String {
    let mut h = Fnv::new();
    h.write_field(url);
    h.write_field(key_rev);
    h.write_field(toolchain);
    // lint inputs, order-independent: sort by path first.
    let mut sorted: Vec<&(String, Vec<u8>)> = lint_inputs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, bytes) in sorted {
        h.write_field(path);
        h.write(bytes);
        h.write(&[0]);
    }
    h.hex()
}

/// The cached engine binary for `key`, building it once if missing. Returns
/// the absolute path to the `mockspace` binary.
///
/// The resolved pin carries one or more install attempts (a `version` pin
/// tries crates.io first, then the git tag); the first that succeeds wins.
/// `cargo install --root` locks the install root and installs the binary
/// atomically, so a concurrent launcher either blocks on that lock or finds
/// the finished binary.
pub fn ensure_built(cache_root: &Path, key: &str, resolved: &Resolved) -> Result<PathBuf, String> {
    let root = builds_dir(cache_root).join(key);
    let bin = root.join("bin").join("mockspace");
    if bin.is_file() {
        return Ok(bin);
    }
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("could not create cache dir {}: {e}", root.display()))?;

    eprintln!("mock: building the engine for this pin (once per version) ...");
    let mut failures = Vec::new();
    for attempt in &resolved.attempts {
        let status = Command::new("cargo")
            .arg("install")
            .args(attempt)
            .arg("--root")
            .arg(&root)
            .arg("--force")
            .status()
            .map_err(|e| format!("could not run cargo install: {e}"))?;
        if status.success() {
            if bin.is_file() {
                return Ok(bin);
            }
            failures.push(format!(
                "{attempt:?} reported success but produced no binary"
            ));
        } else {
            failures.push(format!("{attempt:?} failed"));
        }
    }
    Err(format!(
        "could not build the mockspace engine for this pin; nothing was cached.\n  \
         tried, in order:\n    - {}\n  \
         the pin may be wrong, the release may not exist yet, or the build broke.",
        failures.join("\n    - ")
    ))
}

/// Replace this process with the engine, passing the absolute mock dir so cwd
/// is irrelevant, the pin-matched lint-rules dep (so a custom-lint cdylib links
/// identical types), then the caller's forwarded arguments. On unix `exec`
/// never returns on success; it returns only if the exec itself fails.
pub fn exec_engine(
    bin: &Path,
    mock_abs: &Path,
    lint_rules_dep: &str,
    args: &[String],
) -> Result<std::convert::Infallible, String> {
    use std::os::unix::process::CommandExt;
    let err = Command::new(bin)
        .arg("--dir")
        .arg(mock_abs)
        .arg("--mockspace-lint-rules-dep")
        .arg(lint_rules_dep)
        .args(args)
        .exec();
    Err(format!("failed to exec {}: {err}", bin.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin::{Pin, Reference};

    #[test]
    fn cache_root_prefers_xdg() {
        let xdg = std::path::Path::new("/x/cache");
        let r =
            cache_root_from(Some(xdg.as_os_str().to_os_string()), Some("/home/u".into())).unwrap();
        assert_eq!(r, xdg.join("mockspace"));
    }

    #[test]
    fn cache_root_falls_back_to_home() {
        let r = cache_root_from(None, Some("/home/u".into())).unwrap();
        assert_eq!(r, std::path::Path::new("/home/u/.cache/mockspace"));
        // empty XDG is ignored
        let r2 = cache_root_from(Some("".into()), Some("/home/u".into())).unwrap();
        assert_eq!(r2, std::path::Path::new("/home/u/.cache/mockspace"));
    }

    #[test]
    fn cache_root_errors_without_home() {
        assert!(cache_root_from(None, None).is_err());
    }

    #[test]
    fn key_is_deterministic_and_input_sensitive() {
        let a = compute_key("u", "r1", "tc", &[]);
        let b = compute_key("u", "r1", "tc", &[]);
        assert_eq!(a, b);
        assert_ne!(a, compute_key("u", "r2", "tc", &[]));
        assert_ne!(a, compute_key("v", "r1", "tc", &[]));
        // a toolchain change re-keys (the finding-#1 fix)
        assert_ne!(a, compute_key("u", "r1", "tc2", &[]));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn key_lint_inputs_order_independent() {
        let l1 = vec![("a.rs".to_string(), b"aa".to_vec()), ("b.rs".to_string(), b"bb".to_vec())];
        let l2 = vec![("b.rs".to_string(), b"bb".to_vec()), ("a.rs".to_string(), b"aa".to_vec())];
        assert_eq!(
            compute_key("u", "r", "tc", &l1),
            compute_key("u", "r", "tc", &l2)
        );
        // but content-sensitive
        let l3 = vec![("a.rs".to_string(), b"XX".to_vec())];
        assert_ne!(
            compute_key("u", "r", "tc", &l1),
            compute_key("u", "r", "tc", &l3)
        );
    }

    #[test]
    fn built_binary_short_circuits_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let key = "deadbeefdeadbeef";
        let binpath = builds_dir(dir.path()).join(key).join("bin");
        std::fs::create_dir_all(&binpath).unwrap();
        std::fs::write(binpath.join("mockspace"), b"#!/bin/sh\n").unwrap();
        let resolved = crate::pin::resolve(
            &Pin {
                url:       "u".into(),
                reference: Reference::Rev("r".into()),
            },
            dir.path(),
        )
        .unwrap();
        // present -> returns without invoking cargo.
        let got = ensure_built(dir.path(), key, &resolved).unwrap();
        assert_eq!(got, binpath.join("mockspace"));
    }
}
