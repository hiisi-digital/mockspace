#![allow(unused_imports)]
use super::*;

/// The `..`-prefix that climbs from the mock dir back to the repo root,
/// one segment per component of the repo-root-relative mock path. Pure so
/// the depth arithmetic is testable. `"mock"` -> `"../"`, `"a/b"` -> `"../../"`.
pub(crate) fn ascend_prefix(mock_rel: &str) -> String {
    let depth = Path::new(mock_rel)
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count()
        .max(1);
    "../".repeat(depth)
}

/// Write a `mock` alias into `<mock_dir>/.cargo/config.toml` whose
/// `--manifest-path` climbs back to `<repo>/target/mockspace-proxy`, so
/// `cargo mock` run from the mock dir resolves the proxy correctly. The
/// `--dir` value stays the repo-root-relative mock path, which
/// `resolve_mock_dir` handles from either cwd.
pub(crate) fn ensure_mock_local_alias(
    _repo_root: &Path,
    mock_dir: &Path,
    mock_rel: &str,
    actions: &mut Vec<String>,
) {
    let up = ascend_prefix(mock_rel);
    let alias_value =
        format!("run --manifest-path {up}target/mockspace-proxy/Cargo.toml -- --dir {mock_rel}");
    let alias_line = format!("mock = \"{alias_value}\"");

    let config_dir = mock_dir.join(".cargo");
    let config_path = config_dir.join("config.toml");
    let current = fs::read_to_string(&config_path).unwrap_or_default();

    // Idempotent: leave a healthy alias alone.
    for line in current.lines() {
        let t = line.trim();
        if is_mock_alias_line(t) {
            if let Some((_, val)) = t.split_once('=') {
                if val.trim().trim_matches('"') == alias_value {
                    return;
                }
            }
            let updated: Vec<&str> = current
                .lines()
                .map(
                    |l| {
                        if is_mock_alias_line(l) { alias_line.as_str() } else { l }
                    },
                )
                .collect();
            let _ = fs::write(&config_path, updated.join("\n") + "\n");
            actions.push("updated mock-dir cargo mock alias".into());
            return;
        }
    }

    let _ = fs::create_dir_all(&config_dir);
    let new_content = if current.is_empty() {
        format!("[alias]\n{alias_line}\n")
    } else if current.contains("[alias]") {
        current.replacen("[alias]", &format!("[alias]\n{alias_line}"), 1) + ""
    } else {
        format!("{current}\n[alias]\n{alias_line}\n")
    };
    if fs::write(&config_path, &new_content).is_ok() {
        actions.push(format!("wrote mock-dir alias to {}", config_path.display()));
    }
}

pub(crate) fn ensure_cargo_alias(
    repo_root: &Path,
    mock_dir: &Path,
    mockspace_dir: &Path,
    actions: &mut Vec<String>,
) {
    // Generate the proxy crate that delegates to mockspace.
    // Lives in target/ (gitignored), contains the machine-specific dep path.
    ensure_proxy_crate(repo_root, mock_dir, mockspace_dir, actions);

    let config_dir = repo_root.join(".cargo");
    let config_path = config_dir.join("config.toml");

    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());

    // A second alias config under the mock dir, so `cargo mock` also works
    // when run from there. Cargo resolves `--manifest-path` relative to cwd,
    // so the repo-root alias fails from `mock/`; the mock-local alias points
    // back up to the proxy with the right depth. Config merge picks the
    // closer file, so each cwd gets the alias that resolves for it.
    ensure_mock_local_alias(repo_root, mock_dir, &mock_rel, actions);

    // Both paths are relative to repo root: fully portable.
    // The machine-specific mockspace path lives inside the generated
    // proxy crate at target/mockspace-proxy/ (gitignored).
    let alias_value = format!(
        "run --manifest-path target/mockspace-proxy/Cargo.toml -- --dir {}",
        mock_rel,
    );
    let alias_line = format!("mock = \"{alias_value}\"");

    let current = fs::read_to_string(&config_path).unwrap_or_default();

    // Check if alias already exists and is correct.
    for line in current.lines() {
        let trimmed = line.trim();
        if is_mock_alias_line(trimmed) {
            if let Some((_, val)) = trimmed.split_once('=') {
                let val = val.trim().trim_matches('"');
                if val == alias_value {
                    return; // Healthy.
                }
            }
            // Stale: update in place.
            let updated: Vec<&str> = current
                .lines()
                .map(
                    |l| {
                        if is_mock_alias_line(l) { alias_line.as_str() } else { l }
                    },
                )
                .collect();
            let _ = fs::write(&config_path, updated.join("\n") + "\n");
            actions.push(format!("updated cargo mock alias"));
            return;
        }
    }

    // Missing: add.
    let _ = fs::create_dir_all(&config_dir);
    let new_content = if current.is_empty() {
        format!("[alias]\n{alias_line}\n")
    } else if current.contains("[alias]") {
        let mut result = String::new();
        let mut inserted = false;
        for line in current.lines() {
            result.push_str(line);
            result.push('\n');
            if !inserted && line.trim() == "[alias]" {
                result.push_str(&alias_line);
                result.push('\n');
                inserted = true;
            }
        }
        result
    } else {
        format!("{current}\n[alias]\n{alias_line}\n")
    };
    let _ = fs::write(&config_path, &new_content);
    actions.push(format!("wrote cargo mock alias"));
}

// ──────────────────────────────────────────────────────────────────────
// Proxy crate (target/mockspace-proxy/)
// ──────────────────────────────────────────────────────────────────────

/// True when a config line defines the `mock` alias exactly, i.e. its key
/// (the token before `=`) is `mock`. Guards against clobbering a different
/// key that merely starts with `mock`, e.g. `mockfoo = "..."`.
pub(crate) fn is_mock_alias_line(line: &str) -> bool {
    line.trim()
        .split_once('=')
        .map_or(false, |(k, _)| k.trim() == "mock")
}
