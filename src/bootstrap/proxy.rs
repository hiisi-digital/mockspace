#![allow(unused_imports)]
use super::*;

/// Generate a tiny proxy crate in `target/mockspace-proxy/` that depends on
/// mockspace and delegates to `mockspace::run()`.
///
/// The proxy's Cargo.toml contains the machine-specific absolute path to the
/// mockspace source. Since it lives in `target/` (gitignored), the checked-in
/// `.cargo/config.toml` alias can use a portable relative path:
/// `run --manifest-path target/mockspace-proxy/Cargo.toml -- --dir <mock_rel>`
///
/// If `{mock_dir}/lints/` exists and contains `.rs` files, the proxy is
/// generated with custom lint support. Each `.rs` file must define:
/// - `pub fn lint() -> Box<dyn mockspace_lint_rules::Lint>` for per-crate lints
/// - `pub fn cross_lint() -> Box<dyn mockspace_lint_rules::CrossCrateLint>` for cross-crate lints
pub(crate) fn ensure_proxy_crate(
    repo_root: &Path,
    mock_dir: &Path,
    mockspace_dir: &Path,
    actions: &mut Vec<String>,
) {
    let proxy_dir = repo_root.join("target").join("mockspace-proxy");
    let proxy_cargo = proxy_dir.join("Cargo.toml");
    let proxy_src = proxy_dir.join("src");
    let proxy_main = proxy_src.join("main.rs");

    // The mockspace source the proxy should pin. The passed `mockspace_dir` is
    // the RUNNING binary's baked path, which self-perpetuates: a proxy built
    // against an old mockspace re-pins itself to that same old checkout every
    // run, so `cargo mock` can never advance past whatever the proxy was last
    // built with. The consumer's own resolved lock is the authority instead, so
    // the pin tracks what the consumer actually depends on. Network-free: only
    // an already-present git checkout is used, and anything unresolvable falls
    // back to the baked path (the original behaviour).
    let mockspace_dir = &resolve_mockspace_pin(mock_dir, mockspace_dir, actions);

    // In-tree lint files from {mock_dir}/lints/
    let lints_dir = mock_dir.join("lints");
    let custom_lint_files = discover_custom_lint_files(&lints_dir);

    // External lint-pack crates from [lint-crates] in mockspace.toml
    let lint_packs = parse_lint_crates(&mock_dir.join("mockspace.toml"));

    let has_custom_lints = !custom_lint_files.is_empty() || !lint_packs.is_empty();

    let lint_rules_path = mockspace_dir.join("lint-rules");

    let cargo_content = if has_custom_lints {
        let mut out = String::new();
        out.push_str(&format!(
            "[package]\n\
             name = \"mockspace-proxy\"\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [workspace]\n\
             \n\
             [dependencies]\n\
             mockspace = {{ path = \"{}\" }}\n\
             mockspace-lint-rules = {{ path = \"{}\" }}\n",
            mockspace_dir.display(),
            lint_rules_path.display(),
        ));
        for (name, spec) in &lint_packs {
            out.push_str(&format!("{name} = {spec}\n"));
        }
        if !lint_packs.is_empty() {
            // Third-party lint packs depend on mockspace-lint-rules via git
            // (that's the canonical source spec); the proxy has it as a path
            // dep (from cargo's git cache). Without a patch cargo treats them
            // as two different source identities and fails trait-compat unify.
            // Patch both ssh+https variants of the hiisi-digital canonical URL
            // back to the proxy's local path.
            out.push_str(&format!(
                "\n[patch.\"ssh://git@github.com/hiisi-digital/mockspace.git\"]\n\
                 mockspace = {{ path = \"{ms}\" }}\n\
                 mockspace-lint-rules = {{ path = \"{lr}\" }}\n\
                 \n\
                 [patch.\"https://github.com/hiisi-digital/mockspace.git\"]\n\
                 mockspace = {{ path = \"{ms}\" }}\n\
                 mockspace-lint-rules = {{ path = \"{lr}\" }}\n",
                ms = mockspace_dir.display(),
                lr = lint_rules_path.display(),
            ));
        }
        out
    } else {
        format!(
            "[package]\n\
             name = \"mockspace-proxy\"\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [workspace]\n\
             \n\
             [dependencies]\n\
             mockspace = {{ path = \"{}\" }}\n",
            mockspace_dir.display(),
        )
    };

    let main_content = if has_custom_lints {
        generate_custom_lint_main(&custom_lint_files, &lints_dir, &lint_packs)
    } else {
        "fn main() -> std::process::ExitCode {\n\
        \x20   mockspace::run()\n\
        }\n"
        .to_string()
    };

    // Check if already up-to-date.
    let cargo_ok = fs::read_to_string(&proxy_cargo)
        .map(|c| c == cargo_content)
        .unwrap_or(false);
    let main_ok = fs::read_to_string(&proxy_main)
        .map(|c| c == main_content)
        .unwrap_or(false);

    if cargo_ok && main_ok {
        return; // Healthy.
    }

    // Name a mockspace pin change explicitly, since a silent re-pin was the
    // whole confusion this resolver exists to remove.
    let old_pin = fs::read_to_string(&proxy_cargo)
        .ok()
        .and_then(|c| pinned_mockspace_path(&c));
    let new_pin = mockspace_dir.display().to_string();
    if let Some(old) = old_pin {
        if old != new_pin {
            actions.push(format!("re-pinned proxy mockspace: {old} -> {new_pin}"));
            // Discard the built binary, so the next invocation cannot run code
            // from the revision just re-pinned away from.
            //
            // A stale lock is visible; a binary built from a revision the
            // manifest no longer names is not, and it means a landed fix is
            // silently not running. That happened: a dependency-parser fix was
            // on the branch, the lock advanced to it, and the cached binary
            // kept producing the old answer, which looked like the fix not
            // working. Removing the artifact makes the rebuild unconditional
            // rather than left to fingerprinting to notice.
            discard_proxy_binary(&proxy_dir, actions);
        }
    }

    let _ = fs::create_dir_all(&proxy_src);
    let _ = fs::write(&proxy_cargo, &cargo_content);
    let _ = fs::write(&proxy_main, &main_content);
    actions.push("generated target/mockspace-proxy/".into());
}

/// Remove the built proxy binary so the next run must rebuild it.
///
/// Only the executable, never the whole target directory: the rest is
/// dependency compilation that is still valid and expensive to redo. Both
/// profiles are removed, since which one a project builds is its own choice.
pub(crate) fn discard_proxy_binary(proxy_dir: &Path, actions: &mut Vec<String>) {
    let mut removed = false;
    for profile in ["debug", "release"] {
        let bin = proxy_dir
            .join("target")
            .join(profile)
            .join("mockspace-proxy");
        if bin.exists() && fs::remove_file(&bin).is_ok() {
            removed = true;
        }
    }
    if removed {
        actions.push("discarded the built proxy so the new revision is what runs".into());
    }
}

/// The `mockspace = { path = "..." }` value from a proxy Cargo.toml, if present.
pub(crate) fn pinned_mockspace_path(cargo_toml: &str) -> Option<String> {
    for line in cargo_toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("mockspace = { path = \"") {
            if let Some(end) = rest.find('"') {
                return Some(rest[.. end].to_string());
            }
        }
    }
    None
}

/// The mockspace source path the proxy should pin.
///
/// Reads the consumer's mock-workspace `Cargo.lock` for the `mockspace`
/// package's resolved git revision, then locates the matching checkout in
/// cargo's git cache. This makes the proxy track what the consumer actually
/// resolved, rather than the running binary's own (possibly stale) baked path.
///
/// Falls back to `fallback` (the baked path) when the consumer uses a path or
/// registry dependency (no git revision to track), when the lock cannot be
/// read, or when the resolved checkout is not present on disk. Never fetches.
pub(crate) fn resolve_mockspace_pin(
    mock_dir: &Path,
    fallback: &Path,
    actions: &mut Vec<String>,
) -> PathBuf {
    let rev = match mockspace_rev_from_lock(&mock_dir.join("Cargo.lock")) {
        Some(r) => r,
        None => return fallback.to_path_buf(),
    };
    match find_git_checkout("mockspace", &rev) {
        Some(dir) => dir,
        // The lock names a git revision but its checkout is not present (cargo
        // GC, or a fresh clone before the first build). Falling back to the
        // baked path is the exact self-perpetuation this resolver removes, so
        // make the degraded case visible rather than silent.
        None => {
            let short: String = rev.chars().take(7).collect();
            actions.push(format!(
                "re-pin skipped: mockspace {short} checkout absent; keeping baked path"
            ));
            fallback.to_path_buf()
        },
    }
}

/// The full git revision the mock-workspace lock resolved for `mockspace`.
///
/// Returns `None` for a path or registry source (no git revision), or when the
/// lock is absent or unparseable.
pub(crate) fn mockspace_rev_from_lock(lock_path: &Path) -> Option<String> {
    let content = fs::read_to_string(lock_path).ok()?;
    let doc = content.parse::<toml_edit::DocumentMut>().ok()?;
    let packages = doc.get("package")?.as_array_of_tables()?;
    for pkg in packages.iter() {
        let name = pkg.get("name").and_then(|n| n.as_str());
        if name != Some("mockspace") {
            continue;
        }
        let source = pkg.get("source").and_then(|s| s.as_str())?;
        // Shape: "git+<url>?<query>#<full-rev>". No '#' means a path/registry
        // source, which this resolver does not track.
        let rev = source.rsplit_once('#').map(|(_, r)| r.to_string());
        return rev.filter(|_| source.starts_with("git+"));
    }
    None
}

/// A git dependency as the lock resolved it: the remote URL, the tracked
/// branch (if the source pins one), and the locked revision.
#[derive(Debug, PartialEq)]
pub(crate) struct GitSource {
    pub(crate) url:    String,
    pub(crate) branch: Option<String>,
    pub(crate) rev:    String,
}

/// Every package in the lock pinned to a git BRANCH, by name.
///
/// A branch pin is a moving target by construction: the whole point of tracking
/// `dev` rather than a tag is that the dependency advances. What makes that
/// dangerous rather than useful is that nothing advances the lock, so a project
/// tracking a branch silently runs whatever revision it first resolved, for
/// however long, while reading as though it follows the branch.
///
/// Tags and exact revisions are excluded deliberately: those are pins someone
/// chose, and advancing them would be overriding a decision rather than
/// honouring one.
///
/// Duplicates by name are kept apart: a lock can hold two entries for one
/// package tracking different branches, and collapsing them would advance the
/// wrong one.
pub(crate) fn branch_tracked_git_deps(lock_path: &Path) -> Vec<(String, GitSource)> {
    let Ok(content) = fs::read_to_string(lock_path) else {
        return Vec::new();
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array_of_tables()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pkg in packages.iter() {
        let Some(name) = pkg.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let Some(source) = pkg.get("source").and_then(|s| s.as_str()) else {
            continue;
        };
        if let Some(src) = parse_git_source(source) {
            if src.branch.is_some() {
                out.push((name.to_string(), src));
            }
        }
    }
    out
}

/// Split a `git+<url>[?<query>]#<rev>` source into url, branch, and rev.
pub(crate) fn parse_git_source(source: &str) -> Option<GitSource> {
    let body = source.strip_prefix("git+")?;
    let (locator, rev) = body.rsplit_once('#')?;
    let (url, query) = match locator.split_once('?') {
        Some((u, q)) => (u, Some(q)),
        None => (locator, None),
    };
    let branch = query.and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("branch=").map(str::to_string))
    });
    Some(GitSource {
        url: url.to_string(),
        branch,
        rev: rev.to_string(),
    })
}

/// Whether `cargo mock` may auto-advance a behind mockspace with `cargo update`.
///
/// Read from `[proxy] auto_update` in mockspace.toml. Defaults to `true`: when
/// the locked mockspace is behind its branch's remote head, an interactive
/// `cargo mock` advances it. Set to `false` to only warn.
pub(crate) fn proxy_auto_update(mockspace_toml: &Path) -> bool {
    let Ok(content) = fs::read_to_string(mockspace_toml) else {
        return true;
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return true;
    };
    doc.get("proxy")
        .and_then(|p| p.get("auto_update"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Locate the cargo git checkout for `name` whose directory matches `rev`.
///
/// Cargo checks a git dependency out to
/// `$CARGO_HOME/git/checkouts/<name>-<hash>/<short-rev>/`, where `<short-rev>`
/// is a prefix of the full revision. Globs those directories (via `read_dir`,
/// no glob crate) and returns the first whose subdirectory name is a prefix of
/// `rev`. Returns `None` when no matching checkout is present. Never fetches.
pub(crate) fn find_git_checkout(name: &str, rev: &str) -> Option<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    find_git_checkout_in(&cargo_home.join("git").join("checkouts"), name, rev)
}

/// [`find_git_checkout`] against an explicit `checkouts` root, so it is
/// testable without mutating the process environment.
pub(crate) fn find_git_checkout_in(checkouts: &Path, name: &str, rev: &str) -> Option<PathBuf> {
    let prefix = format!("{name}-");
    for source_entry in fs::read_dir(checkouts).ok()?.flatten() {
        let source_name = source_entry.file_name();
        let source_name = source_name.to_string_lossy();
        // Cargo names a checkout `<repo>-<16-hex-source-hash>`. Require the
        // suffix to be exactly that hash, so `mockspace-<hash>` does not also
        // match a differently-named repo like `mockspace-stack-lints-<hash>`.
        let is_this_source = source_name
            .strip_prefix(&prefix)
            .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        if !is_this_source {
            continue;
        }
        let Ok(revs) = fs::read_dir(source_entry.path()) else {
            continue;
        };
        for rev_entry in revs.flatten() {
            if !rev_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let short = rev_entry.file_name();
            let short = short.to_string_lossy();
            // The checkout dir name is a prefix of the full revision. The first
            // match wins: a within-source short-rev collision needs a 7-hex
            // clash (astronomically unlikely), and two sources containing the
            // same rev are checkouts of the same commit, so either is
            // content-identical.
            if !short.is_empty() && rev.starts_with(short.as_ref()) {
                return Some(rev_entry.path());
            }
        }
    }
    None
}
