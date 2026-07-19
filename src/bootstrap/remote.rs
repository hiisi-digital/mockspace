#![allow(unused_imports)]
use super::*;

/// How long between remote-head checks. The check runs at most once per this
/// interval, so a routine `cargo mock` almost never touches the network.
pub(crate) const REMOTE_CHECK_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);


/// Keep the consumer's locked mockspace current with its branch's remote head.
///
/// When the mock-workspace lock tracks a mockspace git BRANCH and the locked
/// revision is behind that branch's remote head, this advances the lock with
/// `cargo update` (when `allow_update` and `[proxy] auto_update` both permit) or
/// records a warning naming the exact command.
///
/// Called only from an interactive `cargo mock` (never a git hook or a
/// build script), so it never mutates `Cargo.lock` mid-commit and never runs
/// the network in the commit path. Throttled to once per `REMOTE_CHECK_TTL` and
/// offline-safe: a recent prior check, a non-branch source, or any network
/// failure is a silent skip.
pub fn ensure_mockspace_current(
    repo_root: &Path,
    mock_dir: &Path,
    allow_update: bool,
    actions: &mut Vec<String>,
) {
    let deps = branch_tracked_git_deps(&mock_dir.join("Cargo.lock"));
    if deps.is_empty() {
        return; // every dependency is a path, a registry, a tag, or an exact rev.
    }

    let marker = repo_root.join("target/mockspace-proxy/.remote-check");
    if !remote_check_due(&marker, REMOTE_CHECK_TTL) {
        return;
    }
    // Consume the window before the network calls, on purpose: a persistently
    // offline machine then pays the ls-remote timeout at most once per TTL
    // rather than on every `cargo mock`. The cost is that recovery after a
    // transient failure waits until the next window, which is acceptable for a
    // freshness convenience the user can always force with `cargo update`.
    touch(&marker);

    // Auto-advancing mutates the consumer's tracked Cargo.lock. That is a
    // deliberate default (`[proxy] auto_update` defaults to true): tracking a
    // branch is a statement that the dependency should follow it, and a lock
    // that never advances turns that statement into a lie. The mutation is
    // throttled, interactive-only, reversible, and reported with its opt-out.
    let auto = allow_update && proxy_auto_update(&mock_dir.join("mockspace.toml"));

    for (name, source) in deps {
        let Some(branch) = source.branch.clone() else {
            continue;
        };
        let Some(remote) = git_ls_remote_head(&source.url, &branch) else {
            continue; // offline, auth-less, or the ref is gone: skip quietly.
        };
        if remote == source.rev {
            continue; // current.
        }

        let (locked_short, remote_short) = (short_rev(&source.rev), short_rev(&remote));
        if auto {
            match cargo_update_dep(mock_dir, &name, &source) {
                Ok(()) => actions.push(format!(
                    "{name} was behind origin/{branch} ({locked_short} -> {remote_short}); ran cargo update (set [proxy] auto_update = false to only warn)"
                )),
                Err(e) => actions.push(format!(
                    "{name} is behind origin/{branch} ({locked_short} -> {remote_short}); cargo update failed: {e}; run cargo update with the full source spec manually"
                )),
            }
        } else {
            actions.push(format!(
                "{name} is behind origin/{branch} (locked {locked_short}, remote {remote_short})"
            ));
        }
    }
}


/// First seven characters of a git revision, for readable log lines.
pub(crate) fn short_rev(rev: &str) -> String {
    rev.chars().take(7).collect()
}


/// True when no check has run within `ttl` (marker missing or older than `ttl`).
pub(crate) fn remote_check_due(marker: &Path, ttl: std::time::Duration) -> bool {
    match fs::metadata(marker).and_then(|m| m.modified()) {
        Ok(t) => t.elapsed().map(|e| e > ttl).unwrap_or(true),
        Err(_) => true,
    }
}


/// Record that a check ran now, by writing the marker (creating its dir).
pub(crate) fn touch(marker: &Path) {
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(marker, b"");
}


/// The maximum time a remote-head query may take before it is abandoned.
pub(crate) const LS_REMOTE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);


/// The remote head revision of `branch` at `url`, or `None` on any failure.
///
/// Never prompts and never hangs: `GIT_TERMINAL_PROMPT=0` suppresses credential
/// prompts, the SSH command runs in batch mode with a short connect timeout,
/// and the whole invocation is bounded by [`LS_REMOTE_TIMEOUT`] so a slow or
/// black-holed remote over any transport (including HTTP, which the SSH connect
/// timeout does not cover) returns `None` rather than blocking.
pub(crate) fn git_ls_remote_head(url: &str, branch: &str) -> Option<String> {
    // Defence in depth against argv flag smuggling. The url and branch come from
    // Cargo.lock, normally cargo-generated and trusted, but a crafted lock must
    // not be able to pass a leading-dash value that git parses as an option. The
    // `--` separator ends option parsing; the explicit reject refuses the
    // pathological case outright rather than relying on the separator alone.
    if url.starts_with('-') || branch.starts_with('-') {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=5")
        // Bound an HTTP transfer that stalls below 1 byte/s for 5s; the overall
        // timeout below is the real backstop for any transport.
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1")
        .env("GIT_HTTP_LOW_SPEED_TIME", "5")
        .args(["ls-remote", "--", url, &format!("refs/heads/{branch}")]);
    let output = output_with_timeout(cmd, LS_REMOTE_TIMEOUT)?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line is "<sha>\t<ref>"; take the first field of the first line.
    stdout.split_whitespace().next().map(str::to_string)
}


/// Run `cmd` and return its output, or `None` if it fails to spawn or exceeds
/// `timeout` (in which case the child is killed).
///
/// The child's output is expected to be small (a ref line), so its pipes are
/// drained only after exit; a command that floods stdout is not a use here.
pub(crate) fn output_with_timeout(
    mut cmd: std::process::Command,
    timeout: std::time::Duration,
) -> Option<std::process::Output> {
    use std::process::Stdio;
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}


/// Advance the locked mockspace to its branch head.
///
/// Addressed by the full source spec rather than the bare package name. A lock
/// can hold more than one `mockspace` (a project that once tracked `main` and
/// now tracks `dev` keeps both entries), and cargo rejects the bare name as
/// ambiguous. The auto-advance then reported being behind on every run and
/// never advanced, which reads as the feature not working rather than as one
/// stale lock entry.
pub(crate) fn cargo_update_dep(mock_dir: &Path, name: &str, source: &GitSource) -> Result<(), String> {
    let spec = match &source.branch {
        Some(b) => format!("{}?branch={}#{name}", source.url, b),
        None => format!("{}#{name}", source.url),
    };
    let output = std::process::Command::new("cargo")
        .args(["update", "-p", &spec])
        .current_dir(mock_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

