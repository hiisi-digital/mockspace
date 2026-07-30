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
    Some(
        base.join("mockspace")
            .join(format!("hooks-v{HOOK_VERSION}")),
    )
}

/// Write the durable hooks, deferring to the shared installer.
///
/// Idempotent by fingerprint. Best-effort: a failure leaves the generated in-repo
/// hooks as the gate and is reported, never fatal.
pub(crate) fn ensure_durable_hooks(actions: &mut Vec<String>) -> Option<PathBuf> {
    let dir = durable_hooks_dir()?;
    actions.extend(mockspace_manifest::gate::install_durable_hooks(
        &dir,
        HOOK_VERSION,
    ));
    Some(dir)
}

