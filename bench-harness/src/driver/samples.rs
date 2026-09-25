//! What a cell's samples say about the variants that ran, and the bound on one
//! call that the run is held to.

/// The bound on one call at size `n`: what the configuration names, and where
/// it names nothing, what the routine declares for itself.
///
/// The validation probe, the worker's abort and the subprocess deadline all
/// read this one value, and left unset the deadline falls back to 300 s, which
/// kills every call of a routine that takes longer than that however long it
/// said it would.
pub(super) fn call_bound(
    configured: Option<u64>,
    routine: fn(usize) -> Option<u64>,
    n: usize,
) -> Option<u64> {
    configured.or_else(|| routine(n))
}

/// How the names on a cell's samples compare with the variants that ran.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Names {
    /// One name per variant.
    Whole,
    /// No sample at all, which only a failed or killed worker leaves.
    NoneLeft,
    /// Fewer names than variants: a worker failed or was killed, or two
    /// variants export one name. The samples cannot tell the two apart.
    Fewer { names: usize },
}

/// Read the distinct names off `labels` against `variants` variants run.
pub(super) fn names(labels: &[&str], variants: usize) -> Names {
    let mut distinct: Vec<&str> = labels.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    match distinct.len() {
        0 => Names::NoneLeft,
        k if k < variants => Names::Fewer { names: k },
        _ => Names::Whole,
    }
}

#[cfg(test)]
#[path = "samples/tests.rs"]
mod tests;
