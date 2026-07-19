#![allow(unused_imports)]
use super::*;

/// The cargo bin directory: `$CARGO_HOME/bin` when set, else
/// `$HOME/.cargo/bin`. Where `mock` / `cargo-mock` are installed. Pure
/// over its env args. `None` when neither is set.
pub(crate) fn cargo_bin_dir_from(
    cargo_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    cargo_home
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".cargo")))
        .map(|c| c.join("bin"))
}

pub(crate) fn cargo_bin_dir() -> Option<PathBuf> {
    cargo_bin_dir_from(env::var_os("CARGO_HOME"), env::var_os("HOME"))
}

/// The launcher script installed as both `mock` (short form) and
/// `cargo-mock` (cargo's external-subcommand convention). Generic across
/// repos: it discovers the repo and the proxy at runtime with absolute
/// paths, so it works from any working directory, and self-heals a cleaned
/// proxy before running.
pub(crate) fn gen_launcher_script() -> String {
    format!(
        r##"#!/bin/sh
{MANAGED_MARKER}
# mockspace launcher. Installed by the bootstrap as `mock` and `cargo-mock`.
# Discovers the repo and proxy from known locations, so it runs from any cwd.
set -u

# Walk up for the repo root (.git), falling back to a mockspace.toml.
find_root() {{
    d=$(pwd)
    while [ "$d" != "/" ]; do
        if [ -e "$d/.git" ] || [ -f "$d/mockspace.toml" ]; then
            printf '%s' "$d"
            return 0
        fi
        d=$(dirname "$d")
    done
    return 1
}}

root=$(find_root) || {{
    echo "mock: not inside a git repository or mockspace project." >&2
    exit 1
}}

mockdir=$(git -C "$root" config --local mockspace.mockdir 2>/dev/null || true)
[ -z "$mockdir" ] && mockdir="mock"

proxy="$root/target/mockspace-proxy/Cargo.toml"

if [ ! -f "$proxy" ]; then
    # Proxy missing (target cleaned): rebuild it via the mock crate's build.rs.
    ( cd "$root/$mockdir" && cargo check --quiet --locked ) >/dev/null 2>&1 || true
fi

if [ ! -f "$proxy" ]; then
    echo "mock: proxy not found at $proxy and could not be rebuilt." >&2
    echo "  run once from the repo root:  cd $mockdir && cargo check" >&2
    exit 1
fi

exec cargo run --quiet --manifest-path "$proxy" -- --dir "$root/$mockdir" "$@"
"##
    )
}

/// Install the launcher into the cargo bin dir as `mock` and `cargo-mock`,
/// when that dir exists. Idempotent by fingerprint, executable. Skips
/// silently when there is no cargo bin dir; the `.cargo/config.toml` alias
/// remains the floor.
pub(crate) fn ensure_launcher(actions: &mut Vec<String>) {
    let Some(bin) = cargo_bin_dir() else {
        return;
    };
    if !bin.is_dir() {
        return; // no cargo bin dir; alias is the fallback.
    }

    let content = gen_launcher_script();
    let fingerprint = content_fingerprint(&content);
    let fp_line = format!("{MANAGED_MARKER} v{HOOK_VERSION} fp:{fingerprint:016x}");
    let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);

    for name in ["mock", "cargo-mock"] {
        let path = bin.join(name);
        if path.exists() {
            if let Ok(current) = fs::read_to_string(&path) {
                if current.contains(&fp_line) {
                    continue;
                }
                // Only overwrite something we manage, never a foreign binary.
                if !current.contains(MANAGED_MARKER) {
                    actions.push(format!(
                        "not overwriting existing non-mockspace {}",
                        path.display()
                    ));
                    continue;
                }
            } else {
                // Unreadable (likely a real compiled binary): do not touch.
                actions.push(format!(
                    "not overwriting existing binary {}",
                    path.display()
                ));
                continue;
            }
        }
        if let Err(e) = fs::write(&path, &final_content) {
            actions.push(format!("failed to install launcher {name}: {e}"));
            continue;
        }
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&path, perms);
        }
        actions.push(format!("installed launcher {}", path.display()));
    }
}
