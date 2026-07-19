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
mod hash;
mod pin;

use std::path::Path;
use std::process::ExitCode;

use pin::Pin;

/// The launcher entry, shared by both installed binaries (`cargo-mock` and
/// `mock`). Each bin is a two-line shim over this.
pub fn run_cli() -> ExitCode {
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
    let root = discover::repo_root().ok_or_else(|| {
        "not inside a git repository (no .git found, and MOCK_ROOT is unset)".to_string()
    })?;
    let located = discover::locate(&root);
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

    let pin = resolve_pin(located.as_ref(), &root, &mock_abs)?;
    let cache_root = cache::cache_root()?;
    let resolved = pin.resolve(&cache_root)?;
    let key = cache::compute_key(&pin.url, &resolved.key_rev, &cache::rustc_fingerprint(), &[
    ]);
    let bin = cache::ensure_built(&cache_root, &key, &resolved)?;

    // The engine builds and loads this repo's custom lints itself (into its own
    // target/), using the pin-matched lint-rules dep we pass along; the
    // launcher no longer needs to know about lints.
    cache::exec_engine(&bin, &mock_abs, &resolved.lint_rules_dep, args).map(|_never| ())
}

/// The pin: the `mockspace_version` key in the located `mockspace.toml`
/// (wherever it sits), then the legacy mockspace rev in the mock workspace's
/// `Cargo.lock`, which keeps an un-pinned repo running until it adds one.
fn resolve_pin(
    located: Option<&discover::Located>,
    root: &Path,
    mock_abs: &Path,
) -> Result<Pin, String> {
    if let Some(l) = located
        && let Ok(s) = std::fs::read_to_string(&l.config_path)
        && let Some(p) = Pin::from_mockspace_toml(&s)
    {
        return Ok(p);
    }
    if let Ok(s) = std::fs::read_to_string(mock_abs.join("Cargo.lock"))
        && let Some(p) = Pin::from_legacy_lock(&s)
    {
        return Ok(p);
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
