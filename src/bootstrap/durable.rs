#![allow(unused_imports)]
use super::*;

/// The durable hooks directory in the user config home. This survives a
/// `target/` clean, so `core.hooksPath` points here rather than at the
/// generated (and cleanable) `<mock>/target/hooks`. Version-keyed so
/// several mockspace versions coexist without clobbering each other.
///
/// `<XDG_CONFIG_HOME | $HOME/.config>/mockspace/hooks-v<HOOK_VERSION>`.
/// Returns `None` when neither env var is set (no durable home to use).
pub(crate) fn durable_hooks_dir() -> Option<PathBuf> {
    durable_hooks_dir_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

/// Pure core of [`durable_hooks_dir`]: resolves the config base from the
/// two env values so the resolution is testable without touching process
/// env. `XDG_CONFIG_HOME` (when non-empty) wins, else `$HOME/.config`.
pub(crate) fn durable_hooks_dir_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let base = xdg
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))?;
    Some(
        base.join("mockspace")
            .join(format!("hooks-v{HOOK_VERSION}")),
    )
}

/// Write the durable fallback hooks into the user config home. Idempotent
/// by fingerprint, executable, one dir shared by every repo on this
/// mockspace version. Best-effort: a failure here leaves the generated
/// in-repo hooks as the gate and is reported, never fatal.
pub(crate) fn ensure_durable_hooks(actions: &mut Vec<String>) -> Option<PathBuf> {
    let dir = durable_hooks_dir()?;
    if let Err(e) = fs::create_dir_all(&dir) {
        actions.push(format!(
            "could not create durable hooks dir {}: {e}",
            dir.display()
        ));
        return None;
    }

    for hook_name in HOOK_NAMES {
        let path = dir.join(hook_name);
        let content = gen_durable_hook(hook_name);
        let fingerprint = content_fingerprint(&content);
        let fp_line = format!("{MANAGED_MARKER} v{HOOK_VERSION} fp:{fingerprint:016x}");

        if path.exists() {
            if let Ok(current) = fs::read_to_string(&path) {
                if current.contains(&fp_line) {
                    continue;
                }
            }
        }

        let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);
        if let Err(e) = fs::write(&path, &final_content) {
            actions.push(format!("failed to write durable {hook_name}: {e}"));
            continue;
        }

        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&path, perms);
        }
        actions.push(format!("wrote durable {hook_name} to {}", dir.display()));
    }

    Some(dir)
}

/// The durable gate: a self-contained hook in the user config home that
/// `core.hooksPath` points at, so it is invisible to the repo and survives a
/// `target/` clean. Under the dissolved-proxy model there is no generated
/// `mock/target/hooks/` layer to delegate to, so the durable hook does the
/// whole job itself:
///
/// 1. resolves the repo root and, flexibly (root first, then subdirs hidden
///    first), the `mockspace.toml` and mock dir, mirroring the launcher;
/// 2. discovers the `mock` / `cargo-mock` launcher on PATH;
/// 3. with the launcher present, runs the same staged-scope validation the
///    generated hooks did, calling the launcher instead of the `cargo mock`
///    alias;
/// 4. with NO launcher installed, fails closed for the design surface only:
///    a commit touching the mock dir or the config is blocked with an install
///    hint, and everything outside the surface passes freely.
pub(crate) fn gen_durable_hook(name: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str(MANAGED_MARKER);
    s.push('\n');
    s.push_str(&format!(
        "# mockspace durable gate ({name}). Do not edit; rewritten on each `mock` run.\n\
         # core.hooksPath points here: invisible to the repo, survives a target/ clean.\n"
    ));

    // commit-msg is a standalone byline gate. The launcher / mock-surface logic
    // does not apply to it, and byline enforcement must hold regardless of
    // install state, so it skips the shared prelude entirely.
    if name == "commit-msg" {
        s.push_str("set -u\n\n");
        s.push_str(&byline_commit_msg_body());
        s.push_str("exit 0\n");
        return s;
    }

    // pre-push: capture the pushed-ref lines once and run the byline scan
    // before the prelude, so byline enforcement holds even with no launcher
    // installed. The crate-diff loop in the body reads the same captured lines.
    if name == "pre-push" {
        s.push_str("set -u\nPREPUSH_STDIN=$(cat)\n\n");
        s.push_str(&byline_prepush_scan_body());
        s.push_str(DURABLE_PRELUDE);
        s.push_str(DURABLE_PREPUSH_BODY);
        return s;
    }

    let body = match name {
        "pre-commit" => DURABLE_PRECOMMIT_BODY,
        _ => "\"$launcher\" --lint-only --strict 2>&1 || exit 1\n",
    };
    s.push_str(DURABLE_PRELUDE);
    s.push_str(body);
    s
}

/// Shared prelude: resolve root + config + mock dir, discover the launcher,
/// and handle the no-launcher fail-closed-for-the-surface case. Ends with the
/// launcher present and `MOCK_DIR` set to the mock dir relative to the root.
const DURABLE_PRELUDE: &str = r##"set -u

# resolve the repo root (MOCK_ROOT wins, else the .git ancestor)
root="${MOCK_ROOT:-}"
if [ -z "$root" ] || [ ! -d "$root" ]; then
    root=$(git rev-parse --show-toplevel 2>/dev/null) || {
        echo "mockspace gate: not inside a git repository." >&2
        exit 1
    }
fi

# a top-level `mock_dir = "..."` from a mockspace.toml (stops at the first table)
read_mock_dir() {
    awk '/^[[:space:]]*\[/{exit} /^[[:space:]]*mock_dir[[:space:]]*=/{sub(/^[^=]*=[[:space:]]*/,""); gsub(/"/,""); sub(/[[:space:]].*$/,""); print; exit}' "$1" 2>/dev/null
}

# locate the one mockspace.toml + the mock dir (root, then subdirs hidden-first).
# This is a shell reimplementation of the launcher's `discover::locate`
# (cargo-mock/src/discover.rs); the two MUST stay in sync, including the
# single-config rule: a repo has exactly one mockspace.toml, and more than one
# (root plus subdirs) is a hard error. It exists only for the no-launcher
# fallback path; when the launcher is present, `locate` is authoritative.
cfg=""; mockdir=""; cfgcount=0; cfglist=""
if [ -f "$root/mockspace.toml" ]; then
    cfgcount=$((cfgcount + 1)); cfglist="${cfglist}  $root/mockspace.toml
"
    cfg="$root/mockspace.toml"
    md=$(read_mock_dir "$cfg"); [ -z "$md" ] && md="mock"
    mockdir="$root/$md"
fi
for d in "$root"/.*/ "$root"/*/; do
    [ -d "$d" ] || continue
    base=$(basename "$d")
    case "$base" in .|..|.git|target|node_modules) continue ;; esac
    if [ -f "${d}mockspace.toml" ]; then
        cfgcount=$((cfgcount + 1)); cfglist="${cfglist}  ${d}mockspace.toml
"
        if [ -z "$cfg" ]; then
            cfg="${d}mockspace.toml"
            md=$(read_mock_dir "$cfg"); [ -z "$md" ] && md="."
            if [ "$md" = "." ]; then mockdir="${d%/}"; else mockdir="${d%/}/$md"; fi
        fi
    fi
done
if [ "$cfgcount" -gt 1 ]; then
    echo "mockspace gate: found more than one mockspace.toml; a repo must have exactly one. Remove the extras, keep one:" >&2
    printf '%s' "$cfglist" >&2
    exit 1
fi
mockrel="${mockdir#"$root"/}"
cfgrel="${cfg#"$root"/}"

# discover the launcher (direct `mock` preferred; bypasses any stale alias)
launcher=""
if command -v mock >/dev/null 2>&1; then launcher="mock"
elif command -v cargo-mock >/dev/null 2>&1; then launcher="cargo-mock"
fi

if [ -z "$launcher" ]; then
    # no launcher installed: fail closed for the design surface only.
    surface=""
    [ -n "$mockrel" ] && surface=$(git diff --cached --name-only -- "$mockrel" 2>/dev/null || true)
    if [ -z "$surface" ] && [ -n "$cfgrel" ]; then
        surface=$(git diff --cached --name-only -- "$cfgrel" 2>/dev/null || true)
    fi
    if [ -n "$surface" ]; then
        echo "mockspace gate: this changes the mockspace surface (${mockrel:-mock}), which the gate governs." >&2
        echo "  install the launcher:  cargo install cargo-mock" >&2
        echo "  (changes outside the mockspace surface are unaffected.)" >&2
        exit 1
    fi
    exit 0
fi

MOCK_DIR="$mockrel"
"##;

/// pre-commit validation body (launcher present): scope to the changed crates
/// and run the commit-tier lints. Mirrors the generated pre-commit hook.
const DURABLE_PRECOMMIT_BODY: &str = r##"
STAGED=$(git diff --cached --name-only -- "$MOCK_DIR" 2>/dev/null || true)
[ -z "$STAGED" ] && exit 0
echo "pre-commit: mockspace changes detected, running validation..."
CHANGED_CRATES=$(echo "$STAGED" | grep "^$MOCK_DIR/crates/" | sed "s|^$MOCK_DIR/crates/||" | cut -d/ -f1 | sort -u | tr '\n' ',' | sed 's/,$//' || true)
ARGS=(--lint-only --commit)
if [ -n "$CHANGED_CRATES" ]; then
    STAGED_RS=$(echo "$STAGED" | grep "^$MOCK_DIR/crates/.*\.rs$" || true)
    if [ -z "$STAGED_RS" ]; then
        echo "  crates: $CHANGED_CRATES (doc-only)"
        ARGS+=(--scope "$CHANGED_CRATES" --doc-only)
    else
        echo "  crates: $CHANGED_CRATES"
        ARGS+=(--scope "$CHANGED_CRATES")
    fi
else
    echo "  infrastructure-only (no crate files staged)"
    ARGS+=(--scope infra)
fi
if ! "$launcher" "${ARGS[@]}" 2>&1; then
    echo "" >&2
    echo "BLOCKED: mockspace validation failed." >&2
    exit 1
fi
echo "pre-commit: validation passed."
"##;

/// pre-push validation body (launcher present): compute changed crates across
/// every pushed ref from stdin, then run the strict-tier lints. Mirrors the
/// generated pre-push hook.
const DURABLE_PREPUSH_BODY: &str = r##"
echo "pre-push: running mockspace validation..."
NEW_BRANCH=0
CHANGED_CRATES=""
while IFS=' ' read -r _local_ref local_sha _remote_ref remote_sha; do
    [ -z "$local_sha" ] && continue
    [ "$local_sha" = "0000000000000000000000000000000000000000" ] && continue
    if [ "$remote_sha" = "0000000000000000000000000000000000000000" ]; then NEW_BRANCH=1; break; fi
    if ! git cat-file -e "$remote_sha" 2>/dev/null; then NEW_BRANCH=1; break; fi
    PUSH_CHANGED=$(git diff --name-only "$remote_sha".."$local_sha" -- "$MOCK_DIR/crates/" 2>/dev/null | sed "s|^$MOCK_DIR/crates/||" | cut -d/ -f1 | sort -u | tr '\n' ',' | sed 's/,$//' || true)
    [ -z "$PUSH_CHANGED" ] && continue
    if [ -z "$CHANGED_CRATES" ]; then CHANGED_CRATES="$PUSH_CHANGED"; else CHANGED_CRATES="$CHANGED_CRATES,$PUSH_CHANGED"; fi
done <<< "$PREPUSH_STDIN"
if [ -n "$CHANGED_CRATES" ]; then
    CHANGED_CRATES=$(echo "$CHANGED_CRATES" | tr ',' '\n' | sort -u | grep -v '^$' | tr '\n' ',' | sed 's/,$//')
fi
if grep -rq "Nuked by" "$MOCK_DIR/crates/"*/src/lib.rs 2>/dev/null; then
    echo "  nuked workspace: skipping source checks"
    ARGS=(--lint-only --strict --doc-only)
elif [ "$NEW_BRANCH" = "1" ] || [ -z "$CHANGED_CRATES" ]; then
    echo "  scope: full project"
    ARGS=(--lint-only --strict)
else
    echo "  scope: $CHANGED_CRATES"
    ARGS=(--lint-only --strict --scope "$CHANGED_CRATES")
fi
if ! "$launcher" "${ARGS[@]}" 2>&1; then
    echo "" >&2
    echo "BLOCKED: mockspace validation failed." >&2
    exit 1
fi
echo "pre-push: validation passed."
"##;
