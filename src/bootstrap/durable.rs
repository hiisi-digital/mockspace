#![allow(unused_imports)]
use super::*;

/// The durable hooks directory in the user config home. This survives a
/// `target/` clean, so `core.hooksPath` points here rather than at the
/// generated (and cleanable) `<mock>/target/hooks`. Version-keyed so
/// several mockspace versions coexist without clobbering each other.
///
/// `<XDG_CONFIG_HOME | $HOME/.config>/mockspace/hooks-v<HOOK_VERSION>`.
/// Returns `None` when neither env var is set (no durable home to use).
pub(crate) fn durable_hooks_dir() -> Option<PathBuf> {
    durable_hooks_dir_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}


/// Pure core of [`durable_hooks_dir`]: resolves the config base from the
/// two env values so the resolution is testable without touching process
/// env. `XDG_CONFIG_HOME` (when non-empty) wins, else `$HOME/.config`.
pub(crate) fn durable_hooks_dir_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = xdg
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("mockspace").join(format!("hooks-v{HOOK_VERSION}")))
}


/// Write the durable fallback hooks into the user config home. Idempotent
/// by fingerprint, executable, one dir shared by every repo on this
/// mockspace version. Best-effort: a failure here leaves the generated
/// in-repo hooks as the gate and is reported, never fatal.
pub(crate) fn ensure_durable_hooks(actions: &mut Vec<String>) -> Option<PathBuf> {
    let dir = durable_hooks_dir()?;
    if let Err(e) = fs::create_dir_all(&dir) {
        actions.push(format!("could not create durable hooks dir {}: {e}", dir.display()));
        return None;
    }

    for hook_name in HOOK_NAMES {
        let path = dir.join(hook_name);
        let content = gen_durable_hook(hook_name);
        let fingerprint = content_fingerprint(&content);
        let fp_line = format!("{MANAGED_MARKER} v{HOOK_VERSION} fp:{fingerprint:016x}");

        if path.exists() {
            if let Ok(current) = fs::read_to_string(&path) {
                if current.contains(&fp_line) {
                    continue;
                }
            }
        }

        let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);
        if let Err(e) = fs::write(&path, &final_content) {
            actions.push(format!("failed to write durable {hook_name}: {e}"));
            continue;
        }

        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&path, perms);
        }
        actions.push(format!("wrote durable {hook_name} to {}", dir.display()));
    }

    Some(dir)
}


/// The durable fallback hook: generic across every repo, so it locates
/// the repo's live validator at runtime rather than baking a path in.
/// If the validator is gone (a `target/` clean), it rebuilds it via a
/// `cargo check`, then blocks hard if that fails. Fail closed, never open.
pub(crate) fn gen_durable_hook(name: &str) -> String {
    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# mockspace durable gate. Do not edit; rewritten by the bootstrap.
# core.hooksPath points here so the gate survives a `target/` clean.
set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || {{
    echo "mockspace gate: not inside a git repository; refusing to proceed." >&2
    exit 1
}}

mockdir=$(git -C "$root" config --local mockspace.mockdir 2>/dev/null || true)
[ -z "$mockdir" ] && mockdir="mock"

live="$root/$mockdir/target/hooks/{name}"

if [ -x "$live" ]; then
    exec "$live" "$@"
fi

# The generated validator is missing: target/ was cleaned. Self-heal by
# rebuilding it (build.rs re-runs the bootstrap), then retry.
echo "mockspace gate: validator missing at $live" >&2
echo "mockspace gate: self-healing (cargo check in $mockdir/ regenerates it)..." >&2
if ( cd "$root/$mockdir" && cargo check --quiet --locked ) >/dev/null 2>&1 && [ -x "$live" ]; then
    echo "mockspace gate: restored." >&2
    exec "$live" "$@"
fi

echo "" >&2
echo "mockspace gate: BLOCKED. the validator could not be restored." >&2
echo "  run from the repo root:  cargo mock" >&2
echo "  (or:  cd $mockdir && cargo check )" >&2
echo "ALL {name} OPERATIONS BLOCKED until the mockspace validator is restored." >&2
exit 1
"##
    )
}

