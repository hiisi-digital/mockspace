//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo-mock` / `mock`: the launcher for the mockspace design-round
//! workflow engine.
//!
//! It resolves the engine version a repo pins (root `mockspace.toml`,
//! `mockspace_version = "..."`, mapping to a git tag and a crates.io release),
//! builds that engine once into a shared per-version cache under
//! `~/.cache/mockspace/builds/`, and execs it with the absolute mock dir so
//! the working directory never matters. No proxy crate, no `.cargo` alias, no
//! `build.rs` bootstrap: the launcher is the sole entry.
//!
//! Installed as two binaries from one source: `cargo-mock` (cargo's external
//! subcommand convention, so `cargo mock ...` works) and `mock` (the short
//! direct form).

mod cache;
mod discover;
mod engine;
mod hash;
mod pin;
mod registry;
mod selfupdate;

use std::path::Path;
use std::process::ExitCode;

use mockspace_manifest::gate::HOOK_VERSION;
use pin::{Pin, Reference};

/// Where a resolved pin came from, so the registry can tell a repo that has
/// adopted an explicit `mockspace_*` pin from one still on the legacy
/// `Cargo.lock` fallback (the migration-detection signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinSource {
    Toml,
    Legacy,
}

/// The launcher entry, shared by both installed binaries (`cargo-mock` and
/// `mock`). Each bin is a two-line shim over this.
pub fn run_cli() -> ExitCode {
    // The launcher runs as a child of git hooks, whose exported repo-location
    // GIT_* variables poison every `git` this process (and the engine it
    // spawns) invokes from a different working directory. Drop them first.
    //
    // SAFETY: this is the first statement of both installed binaries' mains;
    // no thread has been spawned yet.
    unsafe { mockspace_manifest::gate::sanitize_git_env() };

    let raw: Vec<String> = std::env::args().collect();
    let forwarded = normalize_args(&raw);
    match run(&forwarded) {
        Ok(()) => ExitCode::SUCCESS, // unreachable when exec succeeds
        Err(e) => {
            eprintln!("mock: {e}");
            ExitCode::FAILURE
        },
    }
}

/// Print where this checkout keeps its mockspace, as shell-assignable lines.
///
/// `discover::locate` is the authority on the search, including the rule that a
/// repository has exactly one `mockspace.toml`. Every other consumer of that
/// answer should ask here instead of walking the tree itself: the git hooks
/// already carry a shell reimplementation that has to be kept in step, and a
/// third copy is how the three drift apart.
///
/// Output is `key=value`, one per line, absolute paths, safe to `eval`:
///
/// ```text
/// root=/path/to/repo
/// config=/path/to/repo/mockspace.toml
/// mock_dir=/path/to/repo/mock
/// ```
///
/// `config` is empty when the repository has a mock directory but no config,
/// which is a real shape: mockspace's own repository is one.
fn locate_query() -> Result<(), String> {
    let root = discover::repo_root().ok_or_else(|| {
        "not inside a git repository (no .git found, and MOCK_ROOT is unset)".to_string()
    })?;
    let located = discover::locate(&root)?;
    let (config, mock_dir) = match located {
        Some(l) => (l.config_path.display().to_string(), l.mock_dir),
        // No config, so the conventional directory if it is there at all. The
        // caller distinguishes by the empty `config`.
        None => (String::new(), root.join("mock")),
    };
    println!("root={}", root.display());
    println!("config={config}");
    if mock_dir.is_dir() {
        println!("mock_dir={}", mock_dir.display());
    } else {
        println!("mock_dir=");
    }
    Ok(())
}

/// The user-facing arguments to forward to the engine.
///
/// Two invocation shapes collapse to one: `mock <args...>` passes `<args...>`;
/// `cargo mock <args...>` is executed by cargo as `cargo-mock mock <args...>`,
/// so a leading `mock` is dropped when we were invoked as `cargo-mock`. Any
/// user-supplied `--dir <x>` is stripped: the launcher owns `--dir` (it always
/// passes the absolute mock dir).
fn normalize_args(raw: &[String]) -> Vec<String> {
    let prog = raw
        .first()
        .map(|p| {
            Path::new(p)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_default();
    let mut rest: Vec<String> = raw.iter().skip(1).cloned().collect();
    if prog == "cargo-mock" && rest.first().map(String::as_str) == Some("mock") {
        rest.remove(0);
    }
    strip_dir_flag(rest)
}

/// Drop a `--dir <value>` pair anywhere in the args.
fn strip_dir_flag(args: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a == "--dir" {
            skip = true;
            continue;
        }
        out.push(a);
    }
    out
}

fn run(args: &[String]) -> Result<(), String> {
    // `--engine <path>` is the launcher's, so it comes off before anything reads
    // the arguments, the same as `--dir`.
    let (engine_override, args) = engine::take_flag(args.to_vec());
    let args = args.as_slice();

    // Answered before anything else: it is a question about this checkout, not
    // a run of the engine, so it skips the self-update and the pin resolution
    // and the build. Anything that needs to know where the mockspace is asks
    // this rather than reimplementing the search, which has already been
    // reimplemented once in the git hooks and must stay in sync there.
    if args.first().map(String::as_str) == Some("locate") {
        return locate_query();
    }

    // Keep the launcher itself current (branch installs only, hourly, opt-out).
    // May reinstall and re-exec into the new binary, never returning.
    //
    // Skipped under `--engine`: that flag says which engine to run, and
    // replacing the launcher underneath a deliberate override is the one moment
    // an automatic update is unwelcome.
    if engine_override.is_none()
        && let Ok(cache_root) = cache::cache_root()
    {
        selfupdate::maybe_self_update(&cache_root);
    }

    let root = discover::repo_root().ok_or_else(|| {
        "not inside a git repository (no .git found, and MOCK_ROOT is unset)".to_string()
    })?;
    // `locate` hard-errors (blocking the run) if the repo has more than one
    // mockspace.toml; `None` means none (legacy fallback), `Some` exactly one.
    let located = discover::locate(&root)?;
    // fall back to the conventional mock dir for a repo that has only a legacy
    // Cargo.lock pin and no mockspace.toml yet.
    let mock_abs = located
        .as_ref()
        .map(|l| l.mock_dir.clone())
        .unwrap_or_else(|| root.join("mock"));

    if located.is_none() && !mock_abs.join("Cargo.lock").exists() {
        return Err(format!(
            "no mockspace.toml found under {} and no legacy Cargo.lock pin",
            root.display()
        ));
    }

    // A retired alias intercepts `cargo mock` before this launcher ever runs
    // under that spelling, so it is refused rather than tolerated: the two
    // spellings must be the same tool.
    if located.is_some() {
        // Both spellings cargo honours: config.toml and the extensionless
        // legacy config. The refusal covers what the retired bootstrap wrote,
        // which was always repo-local; a user's own alias elsewhere is their
        // choice, not an anomaly of ours.
        let candidates = [
            root.join(".cargo").join("config.toml"),
            root.join(".cargo").join("config"),
        ];
        for cargo_cfg in candidates {
            let Ok(cfg) = std::fs::read_to_string(&cargo_cfg) else {
                continue;
            };
            if legacy_alias_present(&cfg) {
                return Err(format!(
                    "a retired `cargo mock` alias sits in {}. Cargo resolves \
                     aliases before external subcommands, so `cargo mock` runs \
                     whatever the alias points at instead of this launcher. \
                     Delete the `mock = ...` line, and the [alias] table if \
                     that empties it, then re-run.",
                    cargo_cfg.display()
                ));
            }
        }
    }

    // Plant the durable gate BEFORE building the engine.
    //
    // The engine used to be the only thing that installed it, which leaves a
    // window nothing covers: every way the engine can fail to run also leaves the
    // repo ungated, silently. Its build can fail on a bad pin, on no network, or
    // on a compile error in the pinned revision, and it can fail on the repo's own
    // contents, which is not hypothetical: a workspace with no members exited
    // non-zero before reaching any setup. The launcher cannot fail for any of
    // those reasons, so it plants the gate and the engine keeps it current.
    //
    // Best-effort and quiet on success: a gate that cannot be written is worth
    // reporting, but it must not stop the command the user actually ran.
    if located.is_some() {
        plant_gate(&root);
    }

    let cache_root = cache::cache_root()?;

    // The scratch path: build the engine from a checkout on disk and run this
    // repo's gate against it. No pin is resolved, nothing is recorded in the
    // registry, and nothing is keyed by revision, because the source is a
    // working tree and the question being asked is what it does right now.
    if let Some(raw) = engine_override {
        let source = engine::locate(&raw)?;
        eprintln!(
            "mock: ENGINE OVERRIDE: running the engine at {} instead of this \
             repo's pinned engine",
            source.display()
        );
        let bin = engine::build(&cache_root, &source)?;
        let dep = engine::lint_rules_dep(&source);
        return cache::exec_engine(&bin, &mock_abs, &dep, args).map(|_never| ());
    }

    let (pin, source) = resolve_pin(located.as_ref(), &root, &mock_abs)?;
    let resolved = pin::resolve(&pin, &cache_root)?;
    let toolchain = cache::rustc_fingerprint();
    let key = cache::compute_key(&pin.url, &resolved.key_rev, &toolchain, &[]);
    let bin = cache::ensure_built(&cache_root, &key, &resolved)?;

    // Record this repo + build in the global registry and, at most once a day,
    // garbage-collect engine builds nothing pins anymore. Best-effort: the
    // registry is a cache, never a reason to fail a `mock` run.
    record_and_gc(
        &cache_root,
        &root,
        &mock_abs,
        &pin,
        source,
        &resolved,
        &toolchain,
        &key,
    );

    // A concurrent launcher's GC pass protects only *its own* resolved key, so
    // it could have evicted this build in the window between our build and this
    // exec. Re-materialise it if so, so a background GC never fails a `mock`
    // run (the best-effort registry invariant).
    let bin = if bin.is_file() {
        bin
    } else {
        cache::ensure_built(&cache_root, &key, &resolved)?
    };

    // The engine builds and loads this repo's custom lints itself (into its own
    // target/), using the pin-matched lint-rules dep we pass along; the
    // launcher no longer needs to know about lints.
    cache::exec_engine(&bin, &mock_abs, &resolved.lint_rules_dep, args).map(|_never| ())
}

fn legacy_alias_present(config: &str) -> bool {
    let mut in_alias = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_alias = t == "[alias]";
            continue;
        }
        if in_alias
            && (t.starts_with("mock =")
                || t.starts_with("mock=")
                || t.starts_with("\"mock\" =")
                || t.starts_with("\"mock\"="))
        {
            return true;
        }
    }
    false
}

/// Install the durable hooks and point `core.hooksPath` at them.
///
/// The hook version is the launcher's own, and is deliberately shared with the
/// engine through `mockspace-manifest` rather than duplicated: two copies of a
/// version number is how a repo ends up with hooks from one era wired by another.
/// Whether the repo's `.cargo/config.toml` still carries the retired
/// `cargo mock` alias.
///
/// Cargo resolves aliases before external subcommands, so a leftover alias
/// shadows this launcher whenever `cargo mock` is typed: the user runs
/// whatever the alias points at, not the pinned engine, and nothing says so.
/// Per the anomalous-state rule that is an error with guidance, never a
/// silent difference between `mock` and `cargo mock`.
fn plant_gate(root: &Path) {
    let Some(dir) = mockspace_manifest::gate::durable_hooks_dir(HOOK_VERSION) else {
        return; // no home directory to write into; nothing to do
    };
    let mut actions = mockspace_manifest::gate::install_durable_hooks(&dir, HOOK_VERSION);
    // The same opt-out the engine honours; without it here the launcher edited
    // `core.hooksPath` on every invocation and the variable was inert on the
    // normal path. Hooks still get written; they are inert files until wired.
    if std::env::var("MOCKSPACE_NO_AUTO_ACTIVATE").is_ok() {
        for a in actions {
            eprintln!("mock: {a}");
        }
        return;
    }
    actions.extend(mockspace_manifest::gate::activate(root, &dir));
    for a in actions {
        eprintln!("mock: {a}");
    }
}

/// Unix seconds now, or 0 if the clock is before the epoch (impossible in
/// practice; the registry treats 0 as "very old").
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The registry pin form and value for a resolved pin. Legacy overrides the
/// reference variant, since a legacy pin is always a `Cargo.lock` rev but must
/// register as `legacy` for migration detection.
fn pin_form_and_value(pin: &Pin, source: PinSource) -> (registry::PinForm, String) {
    let value = match &pin.reference {
        Reference::Version(v) | Reference::Branch(v) | Reference::Rev(v) | Reference::Tag(v) => {
            v.clone()
        },
    };
    let form = match source {
        PinSource::Legacy => registry::PinForm::Legacy,
        PinSource::Toml => {
            match &pin.reference {
                Reference::Version(_) => registry::PinForm::Version,
                Reference::Branch(_) => registry::PinForm::Branch,
                Reference::Rev(_) => registry::PinForm::Rev,
                Reference::Tag(_) => registry::PinForm::Tag,
            }
        },
    };
    (form, value)
}

/// Record this repo + its resolved build in the global registry, then run a
/// throttled GC pass protecting the just-resolved key. Every step is
/// best-effort; a registry failure never blocks the engine exec.
#[allow(clippy::too_many_arguments)]
fn record_and_gc(
    cache_root: &Path,
    root: &Path,
    mock_abs: &Path,
    pin: &Pin,
    source: PinSource,
    resolved: &pin::Resolved,
    toolchain: &str,
    key: &str,
) {
    let path = registry::registry_path(cache_root);
    let mut reg = registry::Registry::load(&path);
    let now = now_secs();
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let (form, value) = pin_form_and_value(pin, source);
    reg.record(
        &root.display().to_string(),
        &name,
        &mock_abs.display().to_string(),
        &pin.url,
        form,
        &value,
        key,
        &resolved.key_rev,
        toolchain,
        now,
    );
    if reg.gc_due(now) {
        let removed = reg.gc(cache_root, key, now);
        if !removed.is_empty() {
            eprintln!(
                "mock: cache gc removed {} unused engine build(s)",
                removed.len()
            );
        }
    }
    reg.save(&path);
}

/// The pin: the `mockspace_version` key in the located `mockspace.toml`
/// (wherever it sits), then the legacy mockspace rev in the mock workspace's
/// `Cargo.lock`, which keeps an un-pinned repo running until it adds one.
fn resolve_pin(
    located: Option<&discover::Located>,
    root: &Path,
    mock_abs: &Path,
) -> Result<(Pin, PinSource), String> {
    if let Some(l) = located
        && let Ok(s) = std::fs::read_to_string(&l.config_path)
        && let Some(p) = mockspace_manifest::pin_from_mockspace_toml(&s)
    {
        return Ok((p, PinSource::Toml));
    }
    if let Ok(s) = std::fs::read_to_string(mock_abs.join("Cargo.lock"))
        && let Some(p) = mockspace_manifest::pin_from_legacy_lock(&s)
    {
        return Ok((p, PinSource::Legacy));
    }
    let where_to = located
        .map(|l| l.config_path.clone())
        .unwrap_or_else(|| root.join("mockspace.toml"));
    Err(format!(
        "no mockspace pin found. add one to {}:\n\n    \
         mockspace_version = \"0.0.0-d05\"   # the released engine version\n",
        where_to.display()
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_retired_alias_is_detected_only_under_its_table() {
        assert!(legacy_alias_present("[alias]\nmock = \"run --quiet\"\n"));
        assert!(legacy_alias_present("[build]\njobs = 4\n[alias]\nmock=\"x\"\n"));
        assert!(!legacy_alias_present("[env]\nmock = \"not an alias\"\n"));
        assert!(!legacy_alias_present("[alias]\nmockery = \"other\"\n"));
        assert!(!legacy_alias_present(""));
    }

    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn direct_mock_forwards_all() {
        let raw = s(&["/usr/bin/mock", "lock", "--foo"]);
        assert_eq!(normalize_args(&raw), s(&["lock", "--foo"]));
    }

    #[test]
    fn cargo_mock_drops_leading_mock() {
        let raw = s(&["/root/.cargo/bin/cargo-mock", "mock", "lock", "--foo"]);
        assert_eq!(normalize_args(&raw), s(&["lock", "--foo"]));
    }

    #[test]
    fn cargo_mock_without_subcommand() {
        let raw = s(&["cargo-mock", "mock"]);
        assert_eq!(normalize_args(&raw), Vec::<String>::new());
    }

    #[test]
    fn user_dir_flag_is_stripped() {
        let raw = s(&["mock", "check", "--dir", "/somewhere", "--scope", "x"]);
        assert_eq!(normalize_args(&raw), s(&["check", "--scope", "x"]));
    }
}
