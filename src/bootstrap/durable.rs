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

/// The durable gate: a machine-global hook in the user config home that
/// `core.hooksPath` points at, so it is invisible to the repo and survives a
/// `target/` clean.
///
/// One body serves every repo on this mockspace version, which is what fixes its
/// responsibilities. It carries **no policy of its own**, because policy is
/// per-repo and this file is not. It does exactly three things:
///
/// 1. **Not a mockspace project** (no `mockspace.toml` anywhere): exit 0. The
///    gate has no opinion about a repo it does not govern.
/// 2. **Initialised** (the generated per-repo hook exists and is executable):
///    delegate to it and exit with its status. The generated hook is the one
///    that knows this repo's attribution policy, lint tiers and scope, because
///    those were baked into it when it was written.
/// 3. **Not initialised**: block, at the scope the repo configures via the
///    top-level `uninitialised_blocks` key (`surface`, the default, blocks only
///    changes touching the mock dir or `mockspace.toml`; `all` blocks every
///    commit and push).
///
/// Delegating rather than re-implementing is the point. An earlier shape carried
/// a second copy of the staged-scope validation and of the byline check, with a
/// comment conceding the two copies "MUST stay in sync". They could not, and the
/// byline copy was hardcoded where the generated one was configurable, so a repo
/// configured for autonomous work had its commits demanded by one layer and
/// rejected by the other.
pub(crate) fn gen_durable_hook(name: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str(MANAGED_MARKER);
    s.push('\n');
    s.push_str(&format!(
        "# mockspace durable gate ({name}). Do not edit; rewritten on each `mock` run.\n\
         # core.hooksPath points here: invisible to the repo, survives a target/ clean.\n\
         # Carries no policy. Delegates to the generated per-repo hook when mockspace\n\
         # is initialised, and blocks at the configured scope when it is not.\n"
    ));

    s.push_str("set -u\n");
    // pre-push receives its ref lines on stdin. Capture them before anything
    // else, so both the block path and the delegation can replay them.
    if name == "pre-push" {
        s.push_str("PREPUSH_STDIN=$(cat)\n");
    }
    s.push('\n');
    s.push_str(DURABLE_DISCOVERY);
    s.push_str(&durable_delegate_or_block(name));
    s
}

/// The delegate-or-block decision, per hook name.
///
/// `$1..` are the hook's own arguments (the message-file path for `commit-msg`,
/// the remote name and URL for `pre-push`), forwarded verbatim so the generated
/// hook sees exactly what git passed.
fn durable_delegate_or_block(name: &str) -> String {
    // How to detect that this hook's own concern is in play, for the `surface`
    // scope. commit-msg has no staged-file notion of its own, so it defers to
    // the same staged-surface test the pre-commit hook uses.
    let stdin_replay = if name == "pre-push" {
        "printf '%s\\n' \"$PREPUSH_STDIN\" | "
    } else {
        ""
    };
    format!(
        r##"
# --- initialised? then the generated per-repo hook owns this ---
generated="$mockdir/target/hooks/{name}"
if [ -x "$generated" ]; then
    {stdin_replay}"$generated" "$@"
    exit $?
fi

# --- not initialised: block at the configured scope ---
scope=$(ms_read_key "$cfg" uninitialised_blocks)
[ -z "$scope" ] && scope="surface"

if [ "$scope" != "all" ]; then
    # `surface` scope: only changes to the design surface are the gate's
    # business. Work elsewhere in the repo passes untouched.
    surface=""
    [ -n "$mockrel" ] && surface=$(git diff --cached --name-only -- "$mockrel" 2>/dev/null || true)
    if [ -z "$surface" ] && [ -n "$cfgrel" ]; then
        surface=$(git diff --cached --name-only -- "$cfgrel" 2>/dev/null || true)
    fi
    [ -z "$surface" ] && exit 0
fi

echo "" >&2
echo "BLOCKED: mockspace is not initialised in this repo, so the {name} gate cannot run." >&2
echo "" >&2
echo "  expected the generated hook at:" >&2
echo "    $generated" >&2
echo "" >&2
if [ "$scope" = "all" ]; then
    echo "  scope: all (uninitialised_blocks = \"all\"), so every commit and push" >&2
    echo "  is blocked until mockspace is initialised." >&2
else
    echo "  scope: surface (the default), so this is blocked because it changes" >&2
    echo "  \${{mockrel:-mock}} or the mockspace config. Work outside that passes." >&2
fi
echo "" >&2
echo "  to initialise:  mock" >&2
echo "  if the launcher is missing:  cargo install cargo-mock" >&2
exit 1
"##
    )
}

/// The one shell implementation of mockspace-project discovery.
///
/// Resolves, in order, `root`, `cfg`, `mockdir`, and their repo-relative forms
/// `cfgrel` and `mockrel`, then exits 0 when the repo is not a mockspace project
/// at all. Also defines `ms_read_key`, which reads a top-level scalar from a
/// `mockspace.toml`.
///
/// This mirrors the launcher's `discover::locate`
/// (`cargo-mock/src/discover.rs`), including the single-config rule: a repo has
/// exactly one `mockspace.toml`, and more than one is a hard error. The two are
/// separate implementations in separate languages, so the invariant is stated
/// here and covered by tests rather than by a comment asking a reader to keep
/// them aligned by hand.
///
/// Emitted once and shared by every durable hook, so the walk cannot drift
/// between them.
pub(crate) const DURABLE_DISCOVERY: &str = r##"# resolve the repo root (MOCK_ROOT wins, else the .git ancestor)
root="${MOCK_ROOT:-}"
if [ -z "$root" ] || [ ! -d "$root" ]; then
    root=$(git rev-parse --show-toplevel 2>/dev/null) || {
        echo "mockspace gate: not inside a git repository." >&2
        exit 1
    }
fi

# Read a top-level scalar key from a mockspace.toml. $1 = file, $2 = key.
# Stops at the first table header, so a same-named key inside a [table] is not
# mistaken for the top-level one.
ms_read_key() {
    [ -f "$1" ] || return 0
    awk -v key="$2" '
        /^[[:space:]]*\[/ { exit }
        $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            sub(/^[^=]*=[[:space:]]*/, ""); gsub(/"/, ""); sub(/[[:space:]].*$/, "");
            print; exit
        }' "$1" 2>/dev/null
}

# Locate the one mockspace.toml + the mock dir (root first, then subdirs).
cfg=""; mockdir=""; cfgcount=0; cfglist=""
if [ -f "$root/mockspace.toml" ]; then
    cfgcount=$((cfgcount + 1)); cfglist="${cfglist}  $root/mockspace.toml
"
    cfg="$root/mockspace.toml"
    md=$(ms_read_key "$cfg" mock_dir); [ -z "$md" ] && md="mock"
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
            md=$(ms_read_key "$cfg" mock_dir); [ -z "$md" ] && md="."
            if [ "$md" = "." ]; then mockdir="${d%/}"; else mockdir="${d%/}/$md"; fi
        fi
    fi
done
if [ "$cfgcount" -gt 1 ]; then
    echo "mockspace gate: found more than one mockspace.toml; a repo must have exactly one. Remove the extras, keep one:" >&2
    printf '%s' "$cfglist" >&2
    exit 1
fi

# Not a mockspace project: the gate governs nothing here.
[ -z "$cfg" ] && exit 0

mockrel="${mockdir#"$root"/}"
cfgrel="${cfg#"$root"/}"
"##;
