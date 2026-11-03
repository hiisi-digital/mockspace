//! How long a ref write keeps trying after losing, and what counts as worth another try.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Attempts and the pause between them. The pause before attempt `n + 1` is
/// `n * pause` plus up to as much again of jitter, so writers that collided do
/// not collide again in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub pause:    Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 10,
            pause:    Duration::from_secs(2),
        }
    }
}

impl RetryPolicy {
    /// No pause at all: for tests and for stores that no network sits behind.
    pub fn immediate(attempts: u32) -> Self {
        Self {
            attempts,
            pause: Duration::ZERO,
        }
    }

    pub(crate) fn sleep_after(&self, attempt: u32) {
        let base = self.pause.saturating_mul(attempt);
        if base.is_zero() {
            return;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let jitter = Duration::from_nanos(
            u64::from(nanos ^ std::process::id()) % (base.as_nanos() as u64 + 1),
        );
        std::thread::sleep(base + jitter);
    }
}

/// Whether the words of a failed push name a dropped connection or a ref that
/// stayed locked, which another try can fix. An answer such as a 403, a denial
/// or an authentication failure is final.
pub(crate) fn transient_push_output(out: &str) -> bool {
    let low = out.to_ascii_lowercase();
    const FINAL: [&str; 5] = ["denied", "forbidden", "authentication", "not found", "permission"];
    if FINAL.iter().any(|w| low.contains(w)) {
        return false;
    }
    const TRANSIENT: [&str; 10] = [
        "remote end hung up",
        "early eof",
        "rpc failed",
        "connection reset",
        "connection refused",
        "connection timed out",
        "connection closed",
        "could not resolve",
        "cannot lock ref",
        "http 429",
    ];
    TRANSIENT.iter().any(|w| low.contains(w)) || http_5xx(&low)
}

fn http_5xx(low: &str) -> bool {
    low.match_indices("http 5").any(|(i, _)| {
        let rest = &low.as_bytes()[i + 6 ..];
        rest.len() >= 2
            && rest[.. 2].iter().all(u8::is_ascii_digit)
            && !rest.get(2).is_some_and(u8::is_ascii_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_connections_retry() {
        assert!(transient_push_output(
            "fatal: the remote end hung up unexpectedly"
        ));
        assert!(transient_push_output("error: RPC failed; HTTP 503 curl 22"));
        assert!(transient_push_output(
            "error: cannot lock ref 'refs/mock/x'"
        ));
    }

    #[test]
    fn answers_are_final() {
        assert!(!transient_push_output("remote: Permission denied"));
        assert!(!transient_push_output(
            "fatal: unable to access: The requested URL returned error: 403 forbidden"
        ));
        assert!(!transient_push_output(
            " ! [remote rejected] x -> y (pre-receive hook declined)"
        ));
        assert!(!transient_push_output(""));
    }

    #[test]
    fn a_hex_run_is_not_a_status() {
        assert!(!http_5xx("object http 5abc"));
        assert!(http_5xx("rpc failed; http 502"));
    }
}
