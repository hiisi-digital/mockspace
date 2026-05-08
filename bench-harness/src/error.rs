//! Error type for the bench harness.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Error returned by [`crate::run`] and the harness sub-systems.
///
/// One enum covers the whole harness pipeline (config validation,
/// cdylib load, ABI mismatch, subprocess orchestration, validation,
/// analysis, IO). Subsequent rounds extend the variant set as the
/// pipeline lands.
#[derive(Debug)]
pub enum BenchError {
    /// A piece of the harness has not been ported yet.
    /// Round 1 returns this from the entry point; subsequent rounds
    /// remove individual call sites as the underlying implementation
    /// lands. The string identifies which sub-system is missing so
    /// callers can map the error to a round.
    NotImplemented {
        what: &'static str,
    },
    /// Configuration was rejected before the run started (missing
    /// variant paths, contradictory timing knobs, unknown workload
    /// name, etc.).
    InvalidConfig {
        reason: String,
    },
    /// A variant cdylib could not be loaded via `dlopen`.
    DylibLoadFailed {
        path: PathBuf,
        reason: String,
    },
    /// A variant cdylib's `bench_abi_hash` did not match the harness
    /// `abi_hash()`. Indicates a mismatch between the bench-core
    /// version the variant was built against and the one the harness
    /// is running.
    AbiMismatch {
        path: PathBuf,
        expected: u64,
        found: u64,
    },
    /// Cross-variant validation failed. Carries the offending
    /// (variant, reason) pair; analysis in later rounds may extend
    /// this with the byte-level diff.
    ValidationFailed {
        variant: String,
        reason: String,
    },
    /// A subprocess worker crashed or returned a non-zero exit code.
    WorkerFailed {
        variant: String,
        exit_code: Option<i32>,
        stderr: String,
    },
    /// Generic IO error, wrapped at the boundary so callers do not
    /// need to import [`std::io`].
    Io {
        context: &'static str,
        source: io::Error,
    },
    /// CSV cache parse/write error.
    Cache {
        reason: String,
    },
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BenchError::NotImplemented { what } => {
                write!(f, "not yet implemented: {what}")
            }
            BenchError::InvalidConfig { reason } => {
                write!(f, "invalid bench config: {reason}")
            }
            BenchError::DylibLoadFailed { path, reason } => {
                write!(f, "failed to load variant cdylib {}: {reason}", path.display())
            }
            BenchError::AbiMismatch { path, expected, found } => write!(
                f,
                "ABI mismatch in {}: expected {expected:#x}, found {found:#x}. \
                 Rebuild the variant against the current mockspace-bench-core.",
                path.display()
            ),
            BenchError::ValidationFailed { variant, reason } => {
                write!(f, "validation failed for variant {variant}: {reason}")
            }
            BenchError::WorkerFailed { variant, exit_code, stderr } => match exit_code {
                Some(c) => write!(
                    f,
                    "worker for variant {variant} exited with code {c}\nstderr:\n{stderr}"
                ),
                None => write!(
                    f,
                    "worker for variant {variant} terminated by signal\nstderr:\n{stderr}"
                ),
            },
            BenchError::Io { context, source } => {
                write!(f, "io error during {context}: {source}")
            }
            BenchError::Cache { reason } => {
                write!(f, "cache error: {reason}")
            }
        }
    }
}

impl std::error::Error for BenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BenchError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl BenchError {
    /// Wrap an [`io::Error`] with a static-lifetime context label.
    pub fn io(context: &'static str, source: io::Error) -> Self {
        BenchError::Io { context, source }
    }
}
