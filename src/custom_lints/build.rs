//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Building the generated crate into a cdylib, the same way wherever it runs.
//!
//! **The build is hermetic against the environment.** What a build reads from the process
//! environment (`RUSTFLAGS`, `CARGO_PROFILE_*`, `RUSTC_WRAPPER`, `CARGO_BUILD_*` and the rest of
//! cargo's `CARGO_*` configuration) would change the library the way an edit does, and no identity
//! could list them all. So the child gets only [`KEPT_ENV`] and [`KEPT_ENV_PREFIXES`], which place
//! cargo (where the toolchain and the registry are, the network, the terminal), and the release
//! profile is pinned on the command line, where it outranks every config file. What cargo reads
//! from config files is the one input left, and `identity` folds those files in.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// Variables the build is given as they are: where the toolchain, the registry and the target
/// directory are, how to reach the network, and how the terminal renders.
const KEPT_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "SSH_AUTH_SOCK",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_TERMINAL_PROMPT",
];

/// Families of the same kind, by prefix. `GIT_DIR` and its kin are not among them: a commit hook
/// sets those for the repository being committed to, and a fetch must not follow them.
const KEPT_ENV_PREFIXES: &[&str] =
    &["CARGO_NET_", "CARGO_HTTP_", "CARGO_REGISTRIES_", "CARGO_REGISTRY_", "CARGO_TERM_", "GIT_CONFIG_", "GIT_SSL_"];

/// The release profile as the identity names it, pinned with `--config` so no file or variable
/// moves it.
const PINNED_PROFILE: &[&str] = &[
    "profile.release.opt-level=3",
    "profile.release.debug=false",
    "profile.release.debug-assertions=false",
    "profile.release.overflow-checks=false",
    "profile.release.lto=false",
    "profile.release.panic=\"unwind\"",
    "profile.release.codegen-units=16",
    "profile.release.incremental=false",
    "profile.release.strip=\"none\"",
    "profile.release.rpath=false",
    "build.rustflags=[]",
];

/// Whether the build is given the variable `name`.
pub(crate) fn keeps_env(name: &str) -> bool {
    KEPT_ENV.contains(&name) || KEPT_ENV_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// What a build made, and when it began.
pub(crate) struct Built {
    pub dylib:   PathBuf,
    /// The moment before cargo was started. A source modified after it may or may not have been
    /// read by the compiler, so a library built beside such a source is not kept.
    pub started: SystemTime,
    /// Every file cargo reported making, the library's dependencies among them.
    pub units:   Vec<PathBuf>,
    /// Whether cargo reused an earlier output instead of building, as its own report says.
    pub fresh:   bool,
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
pub(crate) fn build_cdylib(gen_dir: &Path, crate_name: &str) -> Result<Built, String> {
    let started = SystemTime::now();
    // json-render-diagnostics puts artifacts on stdout and still renders
    // warnings and errors to stderr the way a human expects.
    let mut command = Command::new("cargo");
    command.arg("build").arg("--release");
    for setting in PINNED_PROFILE {
        command.arg("--config").arg(setting);
    }
    command
        .arg("--manifest-path")
        .arg(gen_dir.join("Cargo.toml"))
        .arg("--message-format")
        .arg("json-render-diagnostics")
        .env_clear()
        .envs(std::env::vars_os().filter(|(k, _)| k.to_str().is_some_and(keeps_env)))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not run cargo build for the lint cdylib: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cargo build gave no stdout to read artifacts from".to_string())?;
    let mut found = None;
    let mut fresh = false;
    let mut units = Vec::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        units.extend(artifact_files(&line));
        if let Some(p) = cdylib_artifact(&line, crate_name) {
            found = Some(p);
            fresh = artifact_is_fresh(&line);
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
    Ok(Built { dylib, started, units, fresh })
}

/// Whether a `compiler-artifact` line says cargo reused the output.
fn artifact_is_fresh(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|m| m.get("fresh").and_then(|f| f.as_bool()))
        .unwrap_or(false)
}

/// The files a `compiler-artifact` line of cargo's json names, whatever the unit.
fn artifact_files(line: &str) -> Vec<PathBuf> {
    let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else { return Vec::new() };
    if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-artifact") {
        return Vec::new();
    }
    msg.get("filenames")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .collect()
}

/// One line of cargo's json, to the cdylib path it names, or nothing.
///
/// NOTE: cargo normalises a lib target's name, so the package
/// `mockspace-lints-<hash>` arrives here as `mockspace_lints_<hash>`. matching
/// on the package name finds nothing at all, silently, which is why the
/// substitution is done rather than assumed.
pub(crate) fn cdylib_artifact(line: &str, crate_name: &str) -> Option<PathBuf> {
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

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
