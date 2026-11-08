//! Days read from git: when a file was first added, and when it last changed.
//!
//! A document whose name carries no stamp is dated by the commit that added it, and the canon
//! file that holds a row is dated by the last commit that touched it, which is what a tool needs
//! to say a note is older than the canon's last word on its subject.
//!
//! A shallow clone does not know when anything was added, only when the history it kept starts, so
//! its first-added days are refused rather than given as true.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Whether the history here is cut short, so a first-added day would be the cut and not the truth.
#[must_use]
pub fn is_shallow(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-shallow-repository"]).is_some_and(|s| s.trim() == "true")
}

/// The day, since 1970-01-01, of the commit that first added each path, by one pass over the
/// history. A path added more than once is dated by the earliest. Empty where git cannot answer.
#[must_use]
pub fn first_days(root: &Path) -> BTreeMap<String, i64> {
    let Some(log) = git(
        root,
        &["log", "--diff-filter=A", "--no-renames", "--name-only", "--format=@%ct", "-z"],
    ) else {
        return BTreeMap::new();
    };
    let mut out: BTreeMap<String, i64> = BTreeMap::new();
    let mut day: Option<i64> = None;
    for item in log.split('\0') {
        for line in item.split('\n') {
            if let Some(ct) = line.strip_prefix('@') {
                day = ct.trim().parse::<i64>().ok().map(|s| s.div_euclid(86_400));
            } else if !line.is_empty() {
                if let Some(d) = day {
                    let e = out.entry(line.to_string()).or_insert(d);
                    *e = (*e).min(d);
                }
            }
        }
    }
    out
}

/// The day of the last commit that touched a path, or `None` for a path git has no commit for.
#[must_use]
pub fn last_day(root: &Path, rel: &str) -> Option<i64> {
    let s = git(root, &["log", "-1", "--format=%ct", "--", rel])?;
    s.trim().parse::<i64>().ok().map(|s| s.div_euclid(86_400))
}

#[cfg(test)]
mod tests;
