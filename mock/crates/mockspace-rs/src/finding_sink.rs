//! Engine-internal finding collection.
//!
//! Per schema design memo §15: the sink is `Send + Sync` with `&self` emit
//! so rayon-driven per-document parallelism can write through one shared
//! sink. Interior mutability lives in the concrete sink type
//! ([`VecFindingSink`] holds `Mutex<Vec<Finding>>`).
//!
//! Substrate consumers see `Vec<Finding>` at the
//! [`mockspace_core::LintEngine::run`] boundary. Inside the engine, lints
//! emit through the sink during their `check_*` calls; the engine collects
//! and applies suppressions before returning.

use std::sync::Mutex;

use mockspace_core::lint::Finding;

/// Sink that lints emit findings to.
///
/// `Send + Sync` so rayon dispatch can hold one sink across worker threads.
/// `emit` takes `&self` so per-document parallelism does not need to
/// shard sinks per worker.
pub trait FindingSink: Send + Sync {
    fn emit(&self, finding: Finding);
}

/// In-process collector backed by a `Mutex<Vec<Finding>>`. The mutex is
/// uncontended for sequential dispatch and acceptable contention for
/// rayon-driven dispatch at typical run sizes (one lint emits dozens of
/// findings at most; the critical section is a `push`).
#[derive(Debug, Default)]
pub struct VecFindingSink {
    findings: Mutex<Vec<Finding>>,
}

impl VecFindingSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume and return the collected findings.
    pub fn into_findings(self) -> Vec<Finding> {
        self.findings.into_inner().unwrap_or_default()
    }

    /// Snapshot the current findings without consuming the sink.
    pub fn snapshot(&self) -> Vec<Finding> {
        self.findings.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Current finding count without taking ownership.
    pub fn len(&self) -> usize {
        self.findings.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl FindingSink for VecFindingSink {
    fn emit(&self, finding: Finding) {
        // Recover from poison rather than silently dropping the finding:
        // a panicked worker should not cause subsequent emits to vanish.
        let mut guard = self
            .findings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.push(finding);
    }
}

/// Per-run telemetry the engine maintains for reporting. Distinct from
/// the engine's return type (`Vec<Finding>`); engines can expose this
/// through their own accessors when useful.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunReport {
    pub findings_emitted: usize,
    pub findings_after_suppression: usize,
    pub lints_invoked: usize,
    pub lints_skipped: usize,
    pub documents_scanned: usize,
    pub gate_blocked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockspace_core::lint::{Severity, Span};
    use std::borrow::Cow;

    fn dummy_finding(line: u32) -> Finding {
        Finding {
            lint_name: Cow::Borrowed("test"),
            rule_id: None,
            plugin_id: None,
            severity: Severity::Warn,
            impact: None,
            category: None,
            message: Cow::Borrowed("x"),
            span: Span::single_line("a.rs", line, 1, 1),
            fix_suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        }
    }

    #[test]
    fn emit_collects_through_shared_ref() {
        let sink = VecFindingSink::new();
        let sink_ref: &dyn FindingSink = &sink;
        sink_ref.emit(dummy_finding(1));
        sink_ref.emit(dummy_finding(2));
        assert_eq!(sink.len(), 2);
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].span.start_line, 1);
    }

    #[test]
    fn snapshot_does_not_drain() {
        let sink = VecFindingSink::new();
        sink.emit(dummy_finding(1));
        let snap = sink.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(sink.len(), 1);
    }
}
