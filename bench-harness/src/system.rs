//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The system a run is taken on, one of the axes of [`crate::axes`].
//!
//! The machine class says what hardware ran it. What the arms were compiled
//! for is asked of cargo itself rather than read off variables: `cargo rustc
//! --print cfg` under the run's own configuration answers with the target and
//! the target features cargo would hand rustc, whichever of the config files,
//! `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS` or `RUSTFLAGS`
//! set them, by cargo's own precedence. That is compared with what plain
//! `rustc --print cfg` says of the host:
//!
//! - the same target and features: the system is the machine class;
//! - another target: `+target=<arch>-<vendor>-<os>-<env>`;
//! - other features: `+cpu=<name>` where they are the host CPU's own (what
//!   `-C target-cpu=native` gives, named by the CPU it resolved to, so two
//!   CPUs of one class never pool), and `+features=<count>:<digest>` otherwise.
//!
//! `mock bench run` asks once per run and hands the driver the answer in
//! [`SYSTEM_ENV`]; a driver run by hand records its machine class.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::kinds::hex;

/// The environment variable carrying the run's system to the driver.
pub const SYSTEM_ENV: &str = "MOCKSPACE_BENCH_SYSTEM";

/// The keys of a `--print cfg` answer that say what code is generated for.
const TARGET_KEYS: [&str; 5] = ["target_arch", "target_vendor", "target_os", "target_env", "target_abi"];

/// One `key="value"` line's key and value.
fn kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim().trim_matches('"')))
}

/// The target a `--print cfg` answer names, as `arch-vendor-os-env[-abi]`.
#[must_use]
pub fn target_of(cfg: &str) -> String {
    let mut parts = Vec::new();
    for key in TARGET_KEYS {
        if let Some(v) = cfg.lines().filter_map(kv).find(|(k, _)| *k == key).map(|(_, v)| v) {
            if !v.is_empty() {
                parts.push(v.to_string());
            }
        }
    }
    parts.join("-")
}

/// The target features a `--print cfg` answer enables.
#[must_use]
pub fn features_of(cfg: &str) -> BTreeSet<String> {
    cfg.lines()
        .filter_map(kv)
        .filter(|(k, _)| *k == "target_feature")
        .map(|(_, v)| v.to_string())
        .collect()
}

/// The CPU `-C target-cpu=native` resolves to, out of `rustc --print
/// target-cpus` (`native - Select the CPU of the current host (currently X).`).
#[must_use]
pub fn host_cpu(target_cpus: &str) -> Option<String> {
    let line = target_cpus.lines().find(|l| l.trim_start().starts_with("native"))?;
    let at = line.find("currently ")? + "currently ".len();
    Some(line[at ..].trim_end_matches(|c: char| c == ')' || c == '.' || c.is_whitespace()).to_string())
}

/// The system, from the machine class and four answers: plain `rustc
/// --print cfg` (`host`), `cargo rustc --print cfg` under the run's
/// configuration (`effective`), `rustc --print cfg -C target-cpu=native`
/// (`native`), and the CPU `native` resolves to.
#[must_use]
pub fn system(class: &str, host: &str, effective: &str, native: &str, cpu: Option<&str>) -> String {
    let mut parts = vec![class.to_string()];
    if target_of(effective) != target_of(host) {
        parts.push(format!("target={}", target_of(effective)));
    }
    let features = features_of(effective);
    if features != features_of(host) {
        if features == features_of(native) && target_of(effective) == target_of(host) {
            parts.push(format!("cpu={}", cpu.unwrap_or("native")));
        } else {
            let list: Vec<&str> = features.iter().map(String::as_str).collect();
            parts.push(format!("features={}:{}", list.len(), &hex(list.join(",").as_bytes())[.. 12]));
        }
    }
    parts.join("+")
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("spawning {shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{shown} failed:\n{}", String::from_utf8_lossy(&out.stderr).trim_end()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Ask cargo and rustc what the arms of a run are compiled for: `manifest` is
/// a crate the run builds, `config_args` the arguments the run's builds take
/// (its `--config` profile values). The commands run in this process's
/// directory and environment, as the builds do.
pub fn probe(class: &str, manifest: &Path, config_args: &[String]) -> Result<String, String> {
    probe_in(class, manifest, config_args, &std::env::current_dir().unwrap_or_default(), &[])
}

/// [`probe`] with the commands run in `dir`, whose cargo configuration files
/// apply, and with `envs` set on them, a `None` taking the variable away.
/// Cargo reads a `RUSTFLAGS` that is set but empty as no flags, over every
/// configuration file, so leaving one out and emptying it are not the same.
pub fn probe_in(
    class: &str,
    manifest: &Path,
    config_args: &[String],
    dir: &Path,
    envs: &[(&str, Option<&str>)],
) -> Result<String, String> {
    let run = |program: &str, args: &[&str]| {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir).args(args);
        for (k, v) in envs {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        cmd
    };
    let effective = output(
        run("cargo", &["-Zunstable-options", "rustc", "--print", "cfg", "--manifest-path"])
            .arg(manifest)
            .args(config_args),
    )?;
    // The host as rustc sees it with no flags at all, taken outside every
    // configuration the run's cargo reads.
    let bare = |args: &[&str]| {
        let mut cmd = Command::new("rustc");
        cmd.current_dir(dir).args(args).env_remove("RUSTFLAGS");
        cmd
    };
    let host = output(&mut bare(&["--print", "cfg"]))?;
    let native = output(&mut bare(&["--print", "cfg", "-C", "target-cpu=native"]))?;
    let cpus = output(&mut bare(&["--print", "target-cpus"])).unwrap_or_default();
    Ok(system(class, &host, &effective, &native, host_cpu(&cpus).as_deref()))
}

/// The system this driver records: the one `mock bench run` handed it, or
/// the machine class for a driver run by hand.
#[must_use]
pub fn of_this_run(class: &str) -> String {
    std::env::var(SYSTEM_ENV).ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| class.to_string())
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
