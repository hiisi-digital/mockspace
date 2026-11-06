//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a gate run from a git hook says, and where the rest of it goes.
//!
//! A green gate prints nothing. A red one prints `blocked: <gate>`, one line per kind of error
//! with its count, and one line naming the log. Warnings are counted, never listed. Every finding
//! goes to a log under the git directory, one per line as
//! `<severity>\t<lint>\t<path>:<line>\t<message>`, and the whole raw output beside it.
//!
//! The generated hooks capture what `cargo mock` prints into a file and set
//! `MOCKSPACE_GATE_FINDINGS` to a second one, which the lint run fills with its findings. On a
//! failure the hook runs `cargo mock gate-report <gate> <raw> <findings>`, which is this module.
//! `MOCKSPACE_VERBOSE=1` has the hook print the raw output instead, as it always did.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The environment variable naming the file a lint run appends its findings to.
pub const FINDINGS_VAR: &str = "MOCKSPACE_GATE_FINDINGS";

/// The environment variable that prints a hook's whole output.
pub const VERBOSE_VAR: &str = "MOCKSPACE_VERBOSE";

/// How many logs of one gate are kept.
pub const KEEP: usize = 5;

/// One finding of a gate run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: String,
    pub kind:     String,
    pub location: String,
    pub message:  String,
}

impl Finding {
    /// The log line: `<severity>\t<lint>\t<path>:<line>\t<message>`, on one line.
    pub fn line(&self) -> String {
        let one = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        format!(
            "{}\t{}\t{}\t{}",
            one(&self.severity),
            one(&self.kind),
            one(&self.location),
            one(&self.message)
        )
    }

    /// The finding a log line says, or `None` for a line that is not one.
    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.splitn(4, '\t');
        let severity = parts.next()?;
        let kind = parts.next()?;
        let location = parts.next()?;
        let message = parts.next()?;
        if !matches!(severity, "error" | "warn" | "info") {
            return None;
        }
        Some(Self {
            severity: severity.to_string(),
            kind:     kind.to_string(),
            location: location.to_string(),
            message:  message.to_string(),
        })
    }
}

/// Whether the environment asks for the whole output.
pub fn verbose() -> bool {
    std::env::var(VERBOSE_VAR).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Append `findings` to the file the hook named, if it named one. A write that fails is dropped:
/// the raw output holds the same findings.
pub fn record(findings: &[Finding]) {
    let Ok(path) = std::env::var(FINDINGS_VAR) else { return };
    if path.is_empty() || findings.is_empty() {
        return;
    }
    let mut text = String::new();
    for f in findings {
        text.push_str(&f.line());
        text.push('\n');
    }
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// The findings in a findings file, and from raw output where no lint recorded any.
///
/// A gate that is red with no error finding failed on something other than a lint (a dependency
/// check, a missing tool), and says so on a `BLOCKED:` line of the raw output. Those lines become
/// the errors, so the summary is never empty on a red gate.
pub fn collect(findings_text: &str, raw: &str) -> Vec<Finding> {
    let mut all: Vec<Finding> = findings_text.lines().filter_map(Finding::parse).collect();
    if !all.iter().any(|f| f.severity == "error") {
        for line in raw.lines() {
            if let Some(rest) = line.trim().strip_prefix("BLOCKED:") {
                let message = rest.trim();
                let kind = if crate::deny::is_block_message(message) { "deny" } else { "blocked" };
                all.push(Finding {
                    severity: "error".into(),
                    kind:     kind.into(),
                    location: "-:0".into(),
                    message:  message.to_string(),
                });
            }
        }
    }
    if !all.iter().any(|f| f.severity == "error") {
        let last = raw.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output");
        all.push(Finding {
            severity: "error".into(),
            kind:     "failed".into(),
            location: "-:0".into(),
            message:  last.trim().to_string(),
        });
    }
    all
}

/// What a red gate prints: the verdict, a line per kind of error with its count, the number of
/// warnings when there are any, and the log. Nothing else.
pub fn summary(gate: &str, findings: &[Finding], log: &Path) -> String {
    let mut errors: BTreeMap<&str, usize> = BTreeMap::new();
    let mut warnings = 0usize;
    for f in findings {
        match f.severity.as_str() {
            "error" => *errors.entry(f.kind.as_str()).or_default() += 1,
            "warn" => warnings += 1,
            _ => {},
        }
    }
    let mut out = format!("blocked: {gate}\n");
    for (kind, n) in &errors {
        let _ = writeln!(out, "  {kind}: {n}");
    }
    if warnings > 0 {
        let _ = writeln!(out, "  ({warnings} warnings in the log)");
    }
    let _ = writeln!(out, "  log: {}", log.display());
    out
}

/// Write the findings log and the raw log of one run under `dir`, point the gate's `latest` links
/// at them, and prune to the last [`KEEP`]. Returns the findings log.
pub fn write_logs(
    dir: &Path,
    gate: &str,
    stamp: u64,
    findings: &[Finding],
    raw: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let log = dir.join(format!("{gate}-{stamp:012}.log"));
    let raw_path = dir.join(format!("{gate}-{stamp:012}.raw"));
    let mut text = String::new();
    for f in findings {
        text.push_str(&f.line());
        text.push('\n');
    }
    std::fs::write(&log, text)?;
    std::fs::write(&raw_path, raw)?;
    point(&dir.join(format!("{gate}-latest.log")), &log);
    point(&dir.join(format!("{gate}-latest.raw")), &raw_path);
    prune(dir, gate);
    Ok(log)
}

/// Make `link` name `target`, as a symlink built at a name of this run's own and renamed into
/// place, so a link another run is making is never written through.
fn point(link: &Path, target: &Path) {
    #[cfg(unix)]
    {
        let (Some(name), Some(base)) = (target.file_name(), link.file_name()) else { return };
        let mut tmp_name = std::ffi::OsString::from(".tmp-");
        tmp_name.push(std::process::id().to_string());
        tmp_name.push("-");
        tmp_name.push(base);
        let tmp = link.with_file_name(tmp_name);
        let _ = std::fs::remove_file(&tmp);
        if std::os::unix::fs::symlink(name, &tmp).is_ok() && std::fs::rename(&tmp, link).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (link, target);
    }
}

fn prune(dir: &Path, gate: &str) {
    for ext in ["log", "raw"] {
        let prefix = format!("{gate}-");
        let suffix = format!(".{ext}");
        let latest = format!("{gate}-latest.{ext}");
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with(&prefix) && n.ends_with(&suffix) && *n != latest)
            .filter(|n| n[prefix.len() .. n.len() - suffix.len()].bytes().all(|b| b.is_ascii_digit()))
            .collect();
        names.sort();
        let excess = names.len().saturating_sub(KEEP);
        for old in &names[.. excess] {
            let _ = std::fs::remove_file(dir.join(old));
        }
    }
}

fn git_dir() -> Option<PathBuf> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;
    out.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// `mock gate-report <gate> <raw> <findings>`: what a hook runs when its gate was red. Exit 0.
pub fn run(args: &[&str]) -> ExitCode {
    let [gate, raw_path, findings_path] = args else {
        eprintln!("usage: mock gate-report <gate> <raw-output-file> <findings-file>");
        return ExitCode::from(2);
    };
    let raw = std::fs::read_to_string(raw_path).unwrap_or_default();
    let findings_text = std::fs::read_to_string(findings_path).unwrap_or_default();
    if verbose() {
        eprint!("{raw}");
        eprintln!("blocked: {gate}");
        return ExitCode::SUCCESS;
    }
    let findings = collect(&findings_text, &raw);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    let dir = git_dir().map(|g| g.join("mockspace"));
    let log = dir.as_deref().and_then(|d| write_logs(d, gate, stamp, &findings, &raw).ok());
    match log {
        Some(log) => eprint!("{}", summary(gate, &findings, &log)),
        None => {
            // No git directory to write under: the raw output is all there is, so it is shown.
            eprint!("{raw}");
            eprintln!("blocked: {gate}");
        },
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
