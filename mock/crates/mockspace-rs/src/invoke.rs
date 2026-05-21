//! Shared invocation API for finding and spawning the `mock` binary.
//!
//! Per the v2 spec section 57 (see also
//! `mock/research/202605191000_bootstrap-circularity.md`) the binary
//! resolution is a single 5-step priority chain that every caller
//! funnels through. Step 1 (env var override) lands here as the
//! foundation; steps 2-4 (mockspace.toml field, `which mock` on PATH,
//! `cargo mock --version` probe) follow as separate slices of the
//! parent task. Step 5 is the structured error returned when no step
//! resolves a usable binary.
//!
//! The split exists because the spec treats the resolution chain as a
//! single ordered fallback, and writing it all at once invites
//! scope-creep across env / TOML / process / subprocess concerns.
//! Shipping step 1 first locks the public types ([`ResolvedInvocation`]
//! and [`ResolutionError`]) and the dispatch shape so the remaining
//! steps slot in without API churn.

use std::path::PathBuf;

/// Environment variable consulted by step 1 of the resolution chain.
/// If set and pointing at an executable file, this wins outright; the
/// path is baked into hook scripts and used verbatim by every
/// downstream `mockspace_call()` invocation.
pub const ENV_VAR: &str = "MOCKSPACE_BIN_PATH";

/// Result of [`resolve_invocation`]: how the caller should invoke the
/// mock binary. `Absolute` carries the resolved path directly; the
/// caller spawns it with that as argv[0]. `CargoAlias` indicates the
/// chain fell through to the cargo-side resolution (step 4); the
/// caller spawns `cargo mock <args...>` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedInvocation {
    /// Resolved absolute path to the `mock` binary. Steps 1-3 produce
    /// this variant; step 4 produces [`Self::CargoAlias`].
    Absolute(PathBuf),
    /// Cargo-alias form: the resolver found a working `cargo mock`
    /// alias but no standalone binary. The caller spawns `cargo mock`
    /// as a subcommand rather than the binary directly.
    CargoAlias,
}

/// Failure modes for [`resolve_invocation`]. Step 5 is the terminal
/// state when no other step yields a usable binary; the variants
/// describe what each failed step looked like so the diagnostic at
/// the user-facing surface can point at the right remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionError {
    /// `MOCKSPACE_BIN_PATH` was set but did not point at an executable
    /// file. The string is the path the env var carried; the user
    /// sees this in the diagnostic so they can tell the env var is
    /// pointed at the wrong location.
    EnvVarPathNotExecutable(PathBuf),
    /// Every step in the resolution chain failed to produce a usable
    /// binary. The variant carries no payload because every step's
    /// failure is independent; the diagnostic surface composes the
    /// per-step explanations from the spec.
    NoUsablePath,
}

impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvVarPathNotExecutable(p) => {
                write!(
                    f,
                    "{} is set to {} but no executable was found there",
                    ENV_VAR,
                    p.display()
                )
            }
            Self::NoUsablePath => write!(
                f,
                "could not locate the mock binary; set {} or add `mock_bin_path` to mockspace.toml",
                ENV_VAR
            ),
        }
    }
}

impl std::error::Error for ResolutionError {}

/// Walk the 5-step resolution chain. Returns the first step that
/// produces a usable resolution. Per the spec the priority is:
///
/// 1. `MOCKSPACE_BIN_PATH` env var, if set and points at an
///    executable file.
/// 2. `[mockspace] mock_bin_path` in `mockspace.toml`, anchored to
///    the containing file's directory.
/// 3. `which mock` on PATH.
/// 4. `cargo mock --version` probe; on success returns
///    [`ResolvedInvocation::CargoAlias`].
/// 5. [`ResolutionError::NoUsablePath`].
///
/// Steps 2-4 are not yet implemented and currently fall through to
/// step 5; they land as separate slices of task #559. The dispatch
/// shape here is the seam the follow-up slices slot into without
/// touching the public type surface.
pub fn resolve_invocation() -> Result<ResolvedInvocation, ResolutionError> {
    if let Some(path) = resolve_env_var()? {
        return Ok(ResolvedInvocation::Absolute(path));
    }

    // Steps 2-4 land in subsequent slices of task #559. Until then,
    // the chain falls through to the terminal error.
    Err(ResolutionError::NoUsablePath)
}

/// Step 1 of [`resolve_invocation`]: read `MOCKSPACE_BIN_PATH` from
/// the environment. Returns `Ok(Some(path))` when the variable is set
/// and the named path is an executable file, `Ok(None)` when the
/// variable is unset or empty (chain falls through to step 2), and
/// `Err(EnvVarPathNotExecutable)` when the variable is set but the
/// path does not point at an executable file (a configuration
/// mistake the user wants to see surfaced rather than silently
/// skipped).
fn resolve_env_var() -> Result<Option<PathBuf>, ResolutionError> {
    let raw = match std::env::var_os(ENV_VAR) {
        Some(v) => v,
        None => return Ok(None),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(&raw);
    if !is_executable_file(&path) {
        return Err(ResolutionError::EnvVarPathNotExecutable(path));
    }
    Ok(Some(path))
}

/// Best-effort check that `path` names an executable file the current
/// process can spawn. On Unix this checks `is_file()` plus any of the
/// three exec bits (owner / group / other) being set; this is more
/// permissive than "the current user can execute it" but matches the
/// likely intent for a hand-set env var pointing at a binary. On
/// non-Unix platforms it falls back to existence alone, because
/// Windows file-attribute semantics for executability are different
/// and step 1 is rare on Windows anyway: the env var is set by the
/// bootstrap which only writes Unix-shaped paths today.
fn is_executable_file(path: &std::path::Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises tests that mutate the process-wide env var so they
    /// do not race each other. `std::env::set_var` and `remove_var`
    /// are not thread-safe; the unit-test runner runs tests in
    /// parallel by default.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that sets `MOCKSPACE_BIN_PATH` for the duration of
    /// a test and restores the prior value on drop.
    ///
    /// SAFETY: every `set_var` and `remove_var` call below relies on
    /// the caller holding `ENV_LOCK` before constructing the guard.
    /// Tests acquire the mutex at the top of the body; the guard's
    /// `Drop` impl runs while the mutex is still held (it lives
    /// later in the same scope), so no other test thread can observe
    /// or mutate the env var concurrently. Both `set_var` and
    /// `remove_var` are marked `unsafe` on the 2024 edition because
    /// of cross-thread visibility concerns; the mutex enforces the
    /// single-threaded-mutation invariant they require.
    struct EnvVarGuard {
        prior: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(value: &str) -> Self {
            let prior = std::env::var_os(ENV_VAR);
            // SAFETY: see EnvVarGuard struct docs.
            unsafe { std::env::set_var(ENV_VAR, value) };
            Self { prior }
        }

        fn unset() -> Self {
            let prior = std::env::var_os(ENV_VAR);
            // SAFETY: see EnvVarGuard struct docs.
            unsafe { std::env::remove_var(ENV_VAR) };
            Self { prior }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: see EnvVarGuard struct docs; guard outlives the
            // test scope that holds ENV_LOCK so the env var is still
            // serialised at drop time.
            match self.prior.take() {
                Some(v) => unsafe { std::env::set_var(ENV_VAR, v) },
                None => unsafe { std::env::remove_var(ENV_VAR) },
            }
        }
    }

    #[test]
    fn no_env_var_falls_through_to_terminal_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = EnvVarGuard::unset();
        match resolve_invocation() {
            Err(ResolutionError::NoUsablePath) => {}
            other => panic!("expected NoUsablePath, got {other:?}"),
        }
    }

    #[test]
    fn env_var_set_to_existing_executable_resolves_absolute() {
        let _lock = ENV_LOCK.lock().unwrap();
        // /bin/sh stands in for the mock binary; it exists on every
        // Unix system the workspace targets and is unconditionally
        // executable. The resolver does not actually spawn the path;
        // it only checks the executable bit.
        let candidate = std::path::Path::new("/bin/sh");
        if !candidate.exists() {
            // Non-Unix or unusual layout: skip rather than fail.
            return;
        }
        let _g = EnvVarGuard::set("/bin/sh");
        match resolve_invocation() {
            Ok(ResolvedInvocation::Absolute(p)) => {
                assert_eq!(p, PathBuf::from("/bin/sh"));
            }
            other => panic!("expected Absolute(/bin/sh), got {other:?}"),
        }
    }

    #[test]
    fn env_var_pointing_at_nonexistent_path_surfaces_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = EnvVarGuard::set("/nonexistent/mock/binary/path/that/should/not/exist");
        match resolve_invocation() {
            Err(ResolutionError::EnvVarPathNotExecutable(p)) => {
                assert_eq!(
                    p,
                    PathBuf::from("/nonexistent/mock/binary/path/that/should/not/exist")
                );
            }
            other => panic!("expected EnvVarPathNotExecutable, got {other:?}"),
        }
    }

    #[test]
    fn empty_env_var_treated_as_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = EnvVarGuard::set("");
        // Empty value should not fire the "path not executable" error;
        // it should fall through to the next step (currently terminal).
        match resolve_invocation() {
            Err(ResolutionError::NoUsablePath) => {}
            other => panic!("expected NoUsablePath for empty env var, got {other:?}"),
        }
    }

    #[test]
    fn error_display_includes_env_var_name_and_path() {
        let err = ResolutionError::EnvVarPathNotExecutable(PathBuf::from("/some/path"));
        let s = err.to_string();
        assert!(s.contains(ENV_VAR), "Display should mention env var: {s}");
        assert!(s.contains("/some/path"), "Display should include path: {s}");
    }

    #[test]
    fn error_display_no_usable_path_points_at_remediation() {
        let err = ResolutionError::NoUsablePath;
        let s = err.to_string();
        assert!(s.contains(ENV_VAR), "Display should name env var: {s}");
        assert!(
            s.contains("mockspace.toml"),
            "Display should mention TOML fallback: {s}"
        );
    }

    #[test]
    fn env_var_pointing_at_directory_surfaces_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        // A directory is not an executable file; the resolver should
        // surface the type-mismatch through the same error variant.
        let _g = EnvVarGuard::set("/tmp");
        match resolve_invocation() {
            Err(ResolutionError::EnvVarPathNotExecutable(p)) => {
                assert_eq!(p, PathBuf::from("/tmp"));
            }
            other => panic!("expected EnvVarPathNotExecutable for directory, got {other:?}"),
        }
    }

    #[test]
    fn env_var_pointing_at_non_executable_file_surfaces_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        // Create a regular file with no exec bits set. The resolver
        // should surface the type-mismatch through the same error
        // variant the directory test exercises.
        let path = std::env::temp_dir().join(format!(
            "mockspace-invoke-noexec-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"not a binary").expect("write tempfile");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("chmod tempfile");
        }
        let _g = EnvVarGuard::set(path.to_str().expect("utf-8 tempfile path"));
        let observed = resolve_invocation();
        // Clean up before asserting so a failure does not leak the
        // tempfile.
        let _ = std::fs::remove_file(&path);
        #[cfg(unix)]
        match observed {
            Err(ResolutionError::EnvVarPathNotExecutable(p)) => {
                assert_eq!(p, path);
            }
            other => panic!(
                "expected EnvVarPathNotExecutable for non-exec regular file, got {other:?}"
            ),
        }
        #[cfg(not(unix))]
        {
            // On non-Unix the executability check is is_file()-only,
            // so the regular file is treated as executable. Skip the
            // assertion shape but exercise the path.
            let _ = observed;
        }
    }
}
